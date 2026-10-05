//! What a page scan carries from one operator to the next, grouped by what
//! saves, resets or keeps it. `inspect_pinning` owns the loop and the order its
//! operators are matched in; each operator's work is a method on the state it
//! changes, taking the other groups as arguments.
use super::*;
use lopdf::content::Operation;

/// Everything `q` saves and `Q` restores. ISO 32000-1, 8.4.2 and Table 51: the
/// font, its size, the leading and both spacings are graphics state too.
#[derive(Clone)]
pub(super) struct Graphics<'a> {
    // The font and size the last Tf set, and that operator's address.
    pub selected_font: Option<(&'a [u8], f64, usize)>,
    pub leading: f64,
    pub page_transform: [f64; 6],
    pub fill_components: patterns::Colour,
    pub clip: Option<clipping::Rect>,
    pub spacing: f64,
    pub word_spacing: f64,
    pub stroke_components: patterns::Colour,
    pub compound_clips: Vec<clipping::Region>,
    // ISO 32000-1 9.3.6 and 8.4.3.2: the text render mode and the line width
    // are graphics state, saved and restored with it. Modes 1 and 2 stroke the
    // glyphs, so their ink reaches half the line width beyond the outlines.
    pub render: i64,
    pub line_width: f64,
    // Whether a clip the axis-aligned model cannot hold is in force
    // (`clipping::quadrilateral`): text drawn under it is kept read-only.
    pub turned_clip: bool,
}

impl<'a> Graphics<'a> {
    pub fn new(fill_components: patterns::Colour) -> Self {
        Graphics {
            selected_font: None,
            leading: 0.0,
            page_transform: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
            fill_components,
            clip: None,
            spacing: 0.0,
            word_spacing: 0.0,
            stroke_components: patterns::Colour::Solid(1),
            compound_clips: Vec::new(),
            render: 0_i64,
            line_width: 1.0_f64,
            turned_clip: false,
        }
    }

    pub fn save(&self, saved: &mut Vec<Self>) -> Result<(), String> {
        if saved.len() >= 64 {
            return Err("text graphics-state stack exceeds its limit".into());
        }
        saved.push(self.clone());
        Ok(())
    }

    pub fn restore(&mut self, saved: &mut Vec<Self>) -> Result<(), String> {
        *self = saved.pop().ok_or("unmatched graphics-state restore")?;
        Ok(())
    }

    /// A path opened by `re` at `index`: painted rectangles, or a rectangular
    /// clip. Returns the operator the scan resumes at.
    pub fn rectangle(
        &mut self,
        content: &Content,
        index: usize,
        tags: &mut tagging::Tags,
        found: &mut Found,
    ) -> Result<usize, String> {
        let path_until;
        if let Some(rectangles_consumed) =
            clipping::painted(&content.operations[index..], self.page_transform)?
        {
            patterns::paint(
                &content.operations[index + rectangles_consumed - 1].operator,
                self.fill_components,
                self.stroke_components,
            )?;
            if content.operations[index + rectangles_consumed - 1].operator != "n" {
                tags.paint();
            }
            if let Some(bounds) = clipping::drawn(
                &content.operations[index..],
                rectangles_consumed,
                self.page_transform,
            )? {
                found.paths.insert(
                    found.graphics.len(),
                    DrawnPath {
                        operations: (index, index + rectangles_consumed - 1),
                        transform: self.page_transform,
                    },
                );
                found.graphics.push(found.to_display(bounds));
            }
            path_until = index + rectangles_consumed;
        } else if !diagonal(self.page_transform) {
            return Err("non-diagonal clips are not editable yet".into());
        } else {
            self.clip = Some(clipping::apply(
                self.clip,
                &content.operations[index..],
                self.page_transform,
            )?);
            path_until = index + 3;
        }
        Ok(path_until)
    }

    /// A path opened by `m` at `index`: a compound or turned clip, or a
    /// painted path. Returns the operator the scan resumes at.
    pub fn path(
        &mut self,
        content: &Content,
        index: usize,
        tags: &mut tagging::Tags,
        found: &mut Found,
    ) -> Result<usize, String> {
        // A rotated or skewed path may be painted; clipping with one
        // stays refused, because the clip model is axis-aligned.
        if let Some((consumed, region)) = diagonal(self.page_transform)
            .then(|| clipping::compound(&content.operations[index..], self.page_transform))
            .transpose()?
            .flatten()
        {
            if self.compound_clips.len() >= 32 {
                return Err("too many compound clipping intersections".into());
            }
            self.compound_clips.push(region);
            return Ok(index + consumed);
        }
        if let Some(consumed) = diagonal(self.page_transform)
            .then(|| clipping::quadrilateral(&content.operations[index..], self.page_transform))
            .transpose()?
            .flatten()
        {
            self.turned_clip = true;
            return Ok(index + consumed);
        }
        let consumed = clipping::path(&content.operations[index..], self.page_transform)?;
        patterns::paint(
            &content.operations[index + consumed - 1].operator,
            self.fill_components,
            self.stroke_components,
        )?;
        if content.operations[index + consumed - 1].operator != "n" {
            tags.paint();
        }
        if let Some(bounds) =
            clipping::drawn(&content.operations[index..], consumed, self.page_transform)?
        {
            found.paths.insert(
                found.graphics.len(),
                DrawnPath {
                    operations: (index, index + consumed - 1),
                    transform: self.page_transform,
                },
            );
            found.graphics.push(found.to_display(bounds));
        }
        Ok(index + consumed)
    }

    pub fn concatenate(&mut self, values: &[Object]) -> Result<(), String> {
        let mut next = [0.0; 6];
        for (dest, value) in next.iter_mut().zip(values) {
            *dest = number(value)?;
        }
        self.page_transform = compose_affine(self.page_transform, next)?;
        Ok(())
    }

    pub fn external(&mut self, name: &[u8], checked: &mut Checked) -> Result<(), String> {
        if let Some(width) = checked.graphics_state(name)? {
            self.line_width = width;
        }
        Ok(())
    }

    pub fn colour_space(
        &mut self,
        op: &Operation,
        name: &[u8],
        checked: &mut Checked,
    ) -> Result<(), String> {
        let space = checked.colour_space(name)?;
        if op.operator == "cs" {
            self.fill_components = space;
        } else {
            self.stroke_components = space;
        }
        Ok(())
    }

    pub fn fill(&mut self, op: &Operation, values: &[Object]) -> Result<(), String> {
        let components = match op.operator.as_str() {
            "g" => 1,
            "rg" => 3,
            _ => 4,
        };
        self.fill_components = patterns::Colour::Solid(components);
        colors::values(values, components)?;
        Ok(())
    }

    pub fn stroke(&mut self, op: &Operation, values: &[Object]) -> Result<(), String> {
        // Filled text cannot use stroke colour; preserve the validated
        // setter without changing the independently tracked fill space.
        let components = match op.operator.as_str() {
            "G" => 1,
            "RG" => 3,
            _ => 4,
        };
        self.stroke_components = patterns::Colour::Solid(components);
        colors::values(values, components)?;
        Ok(())
    }

    pub fn font(
        &mut self,
        name: &'a Object,
        size: &Object,
        index: usize,
        checked: &mut Checked,
    ) -> Result<(), String> {
        let name = name.as_name().map_err(|e| e.to_string())?;
        checked.font(name)?;
        let size = number(size)?;
        if !(0.0..=1000.0).contains(&size) || size == 0.0 {
            return Err("unsupported text size".into());
        }
        self.selected_font = Some((name, size, index));
        Ok(())
    }
}

/// Where a text object stands: whether one is open, its line matrix, and the
/// cursor each show advances. `BT` resets all of it.
pub(super) struct TextObject {
    pub inside: bool,
    pub positioned: bool,
    pub cursor: f64,
    pub previous_show: Option<u32>,
    // Where the current text line matrix was last set (the BT or Tm), and the
    // leading then in effect: a layout restores the line by replaying from here.
    pub line_origin: (usize, f64),
    pub matrix: [f64; 6],
}

impl TextObject {
    pub fn new() -> Self {
        TextObject {
            inside: false,
            positioned: false,
            cursor: 0.0,
            previous_show: None,
            line_origin: (0_usize, 0.0_f64),
            matrix: [1.0, 0.0, 0.0, 1.0, 0.0, 0.0],
        }
    }

    pub fn begin(&mut self, index: usize, leading: f64) {
        self.inside = true;
        self.positioned = false;
        self.cursor = 0.0;
        self.previous_show = None;
        self.matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
        self.line_origin = (index, leading);
    }

    pub fn set_matrix(
        &mut self,
        values: &[Object],
        index: usize,
        leading: f64,
    ) -> Result<(), String> {
        for (dest, value) in self.matrix.iter_mut().zip(values) {
            *dest = number(value)?;
        }
        compose_affine([1., 0., 0., 1., 0., 0.], self.matrix)?;
        self.positioned = true;
        self.cursor = 0.0;
        self.line_origin = (index, leading);
        Ok(())
    }

    pub fn move_by(
        &mut self,
        op: &Operation,
        x: &Object,
        y: &Object,
        state: &mut Graphics,
    ) -> Result<(), String> {
        let (x, y) = (number(x)?, number(y)?);
        // TD is exactly -ty TL followed by tx ty Td. Both move the
        // line matrix, independently of the preceding show's advance.
        if op.operator == "TD" {
            state.leading = -y;
        }
        move_line(&mut self.matrix, x, y)?;
        self.positioned = true;
        self.cursor = 0.0;
        Ok(())
    }

    pub fn next_line(&mut self, leading: f64) -> Result<(), String> {
        move_line(&mut self.matrix, 0.0, -leading)?;
        self.positioned = true;
        self.cursor = 0.0;
        Ok(())
    }
}

/// The marked-content sequences open at an operator, and the ActualText spans
/// already closed. They balance independently of text objects.
pub(super) struct Marked {
    pub tags: tagging::Tags,
    pub spacer: Option<spacers::Spacer>,
    // An empty spacer outside a text object: open until its EMC.
    pub empty_spacer: bool,
    pub actual: Option<actual::Span>,
    // Inside an optional-content (layer) sequence: its text may be hidden.
    pub layer: bool,
    // Open placed-artwork sequences (`placed_content`): one, or one with its
    // metadata sequence inside. Text in them is kept read-only.
    pub placed: u8,
    pub actual_spans: Vec<actual::Span>,
}

impl Marked {
    pub fn new(tags: tagging::Tags) -> Self {
        Marked {
            tags,
            spacer: None,
            empty_spacer: false,
            actual: None,
            layer: false,
            placed: 0_u8,
            actual_spans: Vec::new(),
        }
    }

    /// One `BDC`, `BMC` or `EMC` at `index`.
    pub fn sequence(
        &mut self,
        op: &Operation,
        index: usize,
        content: &Content,
        object: &TextObject,
        checked: &Checked,
    ) -> Result<(), String> {
        let inside = object.inside;
        if self.actual.is_some() && matches!(op.operator.as_str(), "BDC" | "BMC") {
            return Err("nested ActualText marked content is not editable yet".into());
        }
        if self.layer && matches!(op.operator.as_str(), "BDC" | "BMC") {
            return Err("marked content inside optional content is not editable yet".into());
        }
        let metadata = self.placed == 1
            && op.operator == "BDC"
            && op.operands.first().and_then(|tag| tag.as_name().ok()) == Some(b"Metadata");
        if self.placed > 0 && !metadata && matches!(op.operator.as_str(), "BDC" | "BMC") {
            return Err("marked content inside placed artwork is not editable yet".into());
        }
        match (op.operator.as_str(), op.operands.as_slice()) {
            // Marked content and text objects are independently balanced (ISO
            // 32000-1, 14.6.1). MCIDs use the same ownership checks inside BT;
            // only the narrow ActualText spacer grammar has a separate path.
            // Artifacts balance independently of BT/ET too; PDFMaker opens
            // running headers inside the text object.
            // ISO 32000-1 8.11.3.2: content that belongs to a layer. PowerPoint
            // puts each slide's background in one. The editor never resolves
            // the layer state, so text inside is kept read-only, and nothing
            // may open inside it, which lets the next EMC close it.
            ("BDC", [tag, Object::Name(resource)]) if tag.as_name().ok() == Some(b"OC") => {
                optional_content(checked.doc, checked.resources, resource)?;
                self.layer = true;
            }
            ("EMC", []) if self.layer => self.layer = false,
            ("BDC", [tag, Object::Name(resource)])
                if metadata
                    || (!inside
                        && matches!(tag.as_name().ok(), Some(b"PlacedPDF" | b"PlacedGraphic"))) =>
            {
                placed_content(checked.doc, checked.resources, resource)?;
                self.placed += 1;
            }
            ("EMC", []) if self.placed > 0 => self.placed -= 1,
            ("BDC", [tag, properties])
                if tag.as_name().ok() == Some(b"Artifact")
                    && !properties.as_dict().is_ok_and(|dict| dict.has(b"MCID")) =>
            {
                self.tags.artifact(properties)?
            }
            ("BDC", [tag, properties])
                if !properties.as_dict().is_ok_and(|dict| dict.has(b"MCID")) =>
            {
                let separator = spacers::Spacer::new(tag, properties).ok();
                // InDesign writes a tab stop as an empty span of this kind
                // between text objects, where nothing can be shown: it holds
                // only its ActualText, and there is nothing in it to edit or
                // move.
                let empty = !inside && closes(content, index);
                if let Some(value) = separator.filter(|_| inside || empty) {
                    if inside {
                        self.spacer = Some(value);
                    } else {
                        self.empty_spacer = true;
                    }
                } else {
                    self.actual = Some(actual::Span::new(tag, properties, index)?);
                }
            }
            ("EMC", []) if self.actual.is_some() => {
                if self.actual_spans.len() >= 128 {
                    return Err("too many ActualText spans".into());
                }
                self.actual_spans.push(self.actual.take().unwrap());
            }
            ("EMC", []) if inside && self.spacer.is_some() => {
                self.spacer = None;
            }
            ("EMC", []) if self.empty_spacer => self.empty_spacer = false,
            // An artifact needs no properties, inside a text object or out:
            // LibreOffice marks table-of-contents dot leaders `/Artifact BMC`.
            ("BMC", [tag]) if !inside || tag.as_name().ok() == Some(b"Artifact") => {
                self.tags.begin(tag, None)?
            }
            ("BDC", [tag, properties]) => {
                self.tags.begin(tag, Some(properties))?;
                if closes(content, index) {
                    self.tags.empty();
                }
            }
            ("EMC", []) => self.tags.end()?,
            _ => {
                return Err(refusal::operation(
                    &op.operator,
                    &op.operands,
                    inside,
                    object.positioned || object.previous_show.is_some(),
                ))
            }
        }
        Ok(())
    }
}

/// The page's resources as far as the scan has validated them. A name is
/// checked once, at its first use, and what it was found to be is kept.
pub(super) struct Checked<'a> {
    pub doc: &'a Document,
    pub resources: &'a Dictionary,
    colour_spaces: BTreeMap<Vec<u8>, patterns::Colour>,
    shading_patterns: BTreeSet<Vec<u8>>,
    // Each validated state and the line width it sets, if any.
    graphics_states: BTreeMap<Vec<u8>, Option<f64>>,
    image_names: BTreeSet<Vec<u8>>,
    stencils: BTreeSet<Vec<u8>>,
    form_bounds: BTreeMap<Vec<u8>, Option<[f64; 4]>>,
    image_bytes: usize,
    // What each named XObject paints, in its own space: the unit square for an
    // image (ISO 32000-1 8.9.5.2 maps every image onto it) and the BBox for a
    // preserved form. A name is checked once and drawn many times, each time
    // under its own CTM, so the rectangle is kept and transformed per use.
    drawn_bounds: BTreeMap<Vec<u8>, [f64; 4]>,
    font_metrics: BTreeMap<Vec<u8>, fonts::Metrics>,
    font_boxes: BTreeMap<Vec<u8>, [f64; 4]>,
    // Why the first font the editor cannot write with was kept read-only: the
    // refusal a page gets when that leaves it nothing to edit.
    pub unusable_font: Option<String>,
}

impl<'a> Checked<'a> {
    pub fn new(doc: &'a Document, resources: &'a Dictionary) -> Self {
        Checked {
            doc,
            resources,
            colour_spaces: BTreeMap::new(),
            shading_patterns: BTreeSet::new(),
            graphics_states: BTreeMap::new(),
            image_names: BTreeSet::new(),
            stencils: BTreeSet::new(),
            form_bounds: BTreeMap::new(),
            image_bytes: 0,
            drawn_bounds: BTreeMap::new(),
            font_metrics: BTreeMap::new(),
            font_boxes: BTreeMap::new(),
            unusable_font: None,
        }
    }

    /// The line width the named ExtGState sets, if it sets one.
    fn graphics_state(&mut self, name: &[u8]) -> Result<Option<f64>, String> {
        if !self.graphics_states.contains_key(name) {
            if self.graphics_states.len() >= 32 {
                return Err("too many external text graphics states".into());
            }
            let width = graphics::normal(self.doc, self.resources, name)?;
            self.graphics_states.insert(name.to_vec(), width);
        }
        Ok(self.graphics_states[name])
    }

    fn colour_space(&mut self, name: &[u8]) -> Result<patterns::Colour, String> {
        if !self.colour_spaces.contains_key(name) {
            if self.colour_spaces.len() >= 32 {
                return Err("too many text colour spaces".into());
            }
            self.colour_spaces.insert(
                name.to_vec(),
                if name == b"Pattern" {
                    patterns::Colour::Pattern { selected: false }
                } else {
                    patterns::Colour::Solid(colors::named(self.doc, self.resources, name)?)
                },
            );
        }
        Ok(self.colour_spaces[name])
    }

    fn font(&mut self, name: &[u8]) -> Result<(), String> {
        if !self.font_metrics.contains_key(name) {
            if self.font_metrics.len() >= 32 {
                return Err("too many fonts on an editable page".into());
            }
            // A simple font the editor cannot write with keeps its
            // text read-only (`fonts::read_only`) rather than refusing
            // the page; when even that cannot measure it, the page is
            // refused with the font's own reason.
            let metrics = match font(self.doc, self.resources, name) {
                Ok(metrics) => metrics,
                Err(error) => {
                    let metrics = read_only_font(self.doc, self.resources, name).ok_or(&error)?;
                    self.unusable_font.get_or_insert(error);
                    metrics
                }
            };
            self.font_metrics.insert(name.to_vec(), metrics);
            if let Some(bounds) = standard_box(self.doc, self.resources, name) {
                self.font_boxes.insert(name.to_vec(), bounds);
            }
        }
        Ok(())
    }

    /// A colour set by `sc`, `scn`, `SC` or `SCN` in the space `colour` holds.
    pub fn colour(
        &mut self,
        colour: &mut patterns::Colour,
        op: &Operation,
        values: &[Object],
    ) -> Result<(), String> {
        colour.set(
            self.doc,
            self.resources,
            &op.operator,
            values,
            &mut self.shading_patterns,
        )
    }

    /// One `Do`: the XObject it names is validated and charged to the page's
    /// image budget at its first use, and every use is placed under the
    /// transform and clip then current.
    pub fn draw(&mut self, name: &[u8], state: &Graphics, found: &mut Found) -> Result<(), String> {
        if !self.image_names.contains(name) {
            if self.image_names.len() >= 32 {
                return Err("too many images on an editable page".into());
            }
            if let Some(form) = forms::check(
                self.doc,
                self.resources,
                name,
                MAX_IMAGES - self.image_bytes,
            )? {
                self.image_bytes += form.bytes;
                self.form_bounds.insert(name.to_vec(), form.text_bounds);
                self.drawn_bounds.insert(name.to_vec(), form.bounds);
            } else {
                let image = images::check(
                    self.doc,
                    self.resources,
                    name,
                    MAX_IMAGES - self.image_bytes,
                )?;
                self.image_bytes += image.bytes;
                if image.stencil {
                    self.stencils.insert(name.to_vec());
                }
                self.drawn_bounds.insert(name.to_vec(), [0., 0., 1., 1.]);
            }
            self.image_names.insert(name.to_vec());
        }
        // Where this use of it lands, clipped as its text bounds are.
        if let Some(&painted) = self.drawn_bounds.get(name) {
            let mut bounds = text_bounds(state.page_transform, painted);
            if let Some(clip) = state.clip {
                bounds = [
                    bounds[0].max(clip[0]),
                    bounds[1].max(clip[1]),
                    bounds[2].min(clip[2]),
                    bounds[3].min(clip[3]),
                ];
            }
            if bounds[0] < bounds[2] && bounds[1] < bounds[3] {
                found.graphics.push(found.to_display(bounds));
            }
        }
        // A stencil mask paints the fill colour current at each use.
        if self.stencils.contains(name) {
            patterns::paint("f", state.fill_components, state.stroke_components)?;
        }
        if let Some(Some(bounds)) = self.form_bounds.get(name) {
            let mut bounds = text_bounds(state.page_transform, *bounds);
            if let Some(clip) = state.clip {
                bounds = [
                    bounds[0].max(clip[0]),
                    bounds[1].max(clip[1]),
                    bounds[2].min(clip[2]),
                    bounds[3].min(clip[3]),
                ];
            }
            if bounds[0] < bounds[2] && bounds[1] < bounds[3] {
                found.form_text_bounds.push(found.to_display(bounds));
            }
        }
        Ok(())
    }
}

/// What the scan collects for the `Inspection` it returns.
pub(super) struct Found {
    pub runs: PageRuns,
    pub continued: BTreeSet<u32>,
    pub font_operators: BTreeMap<u32, usize>,
    pub horizontal_bounds: BTreeMap<u32, [f64; 2]>,
    pub blocks: BTreeMap<u32, ObjectId>,
    pub text_spacing: BTreeMap<u32, (f64, f64)>,
    pub leads: BTreeMap<u32, Object>,
    pub gaps: BTreeMap<u32, f64>,
    pub compound_run_clips: BTreeMap<u32, (Vec<clipping::Region>, [f64; 4])>,
    pub contexts: BTreeMap<u32, layout::Context>,
    // Each structure Span with rewritable ActualText: its shows, and whether
    // none of them is inside an ActualText span of its own. A read-only or
    // spacer show never becomes a run, which the comparison in `settle_spans` sees.
    pub span_shows: BTreeMap<ObjectId, (Vec<u32>, bool)>,
    pub preserved: Vec<Run>,
    pub centred: BTreeSet<u32>,
    pub form_text_bounds: Vec<[f32; 4]>,
    pub graphics: Vec<[f32; 4]>,
    pub paths: BTreeMap<usize, DrawnPath>,
    pub sheet: crate::pagetree::DisplayedPage,
}

/// One `Tj` or `TJ` as measured, before its hit box is known.
struct Shown<'a> {
    index: usize,
    name: &'a [u8],
    size: f64,
    font_operator: usize,
    metrics: &'a fonts::Metrics,
    text: String,
    advance: f64,
    horizontal: [f64; 2],
    shown_matrix: [f64; 6],
    page_matrix: [f64; 6],
    read_only: bool,
    // Half the line width, in page units, around stroked glyphs.
    stroke: f64,
}

impl Found {
    pub fn new(page: u32, revision: Vec<u8>, sheet: crate::pagetree::DisplayedPage) -> Self {
        Found {
            runs: PageRuns {
                page,
                // The scan holds one document and cannot name it; the command that
                // chose which document to ask fills this in. See the field.
                source: None,
                revision,
                runs: Vec::new(),
                preview: None,
            },
            continued: BTreeSet::new(),
            font_operators: BTreeMap::new(),
            horizontal_bounds: BTreeMap::new(),
            blocks: BTreeMap::new(),
            text_spacing: BTreeMap::new(),
            leads: BTreeMap::new(),
            gaps: BTreeMap::new(),
            compound_run_clips: BTreeMap::new(),
            contexts: BTreeMap::new(),
            span_shows: BTreeMap::new(),
            preserved: Vec::new(),
            centred: BTreeSet::new(),
            form_text_bounds: Vec::new(),
            graphics: Vec::new(),
            paths: BTreeMap::new(),
            sheet,
        }
    }

    fn to_display(&self, bounds: [f64; 4]) -> [f32; 4] {
        let sheet = &self.sheet;
        let (sox, soy) = (f64::from(sheet.origin.0), f64::from(sheet.origin.1));
        crate::text::to_device(
            sheet.turns,
            sheet.width,
            sheet.height,
            [
                bounds[0] - sox,
                bounds[1] - soy,
                bounds[2] - sox,
                bounds[3] - soy,
            ],
        )
    }

    /// One `Tj` or `TJ` at `index`: measured, bounded, and recorded as an
    /// editable run, a read-only one, or a spacer's show.
    pub fn show(
        &mut self,
        index: usize,
        op: &Operation,
        state: &Graphics,
        object: &mut TextObject,
        marked: &mut Marked,
        checked: &Checked,
    ) -> Result<(), String> {
        marked.tags.text()?;
        if let Some(block) = marked.tags.block() {
            self.blocks.insert(index as u32, block);
        }
        let (name, size, font_operator) = state.selected_font.ok_or("text has no explicit font")?;
        if !object.positioned {
            self.continued.insert(
                object
                    .previous_show
                    .ok_or("text has no preceding position")?,
            );
        }
        object.positioned = false;
        object.previous_show = Some(index as u32);
        let metrics = checked.font_metrics.get(name).ok_or("missing text font")?;
        let (text, advance, horizontal, backtracks) =
            self.measure(index, op, metrics, size, state, object)?;
        let mut shown_matrix = object.matrix;
        shift_position(
            &mut shown_matrix,
            object.cursor * object.matrix[0],
            object.cursor * object.matrix[1],
        )?;
        let page_matrix = if diagonal(state.page_transform) && orthogonal(shown_matrix) {
            compose_orthogonal(state.page_transform, shown_matrix)?
        } else {
            compose_affine(state.page_transform, shown_matrix)?
        };
        // A glyph whose text the editor cannot write keeps its whole run.
        let read_only = marked.tags.read_only()
            || marked.layer
            || state.turned_clip
            || backtracks
            || text.contains(fonts::OPAQUE)
            || !diagonal(state.page_transform)
            || !orthogonal(page_matrix)
            || page_matrix[0] * page_matrix[3] - page_matrix[1] * page_matrix[2] <= 0.0
            || !matches!(state.fill_components, patterns::Colour::Solid(_))
            || (matches!(state.render, 1 | 2)
                && !matches!(state.stroke_components, patterns::Colour::Solid(_)));
        let read_only = read_only || marked.placed > 0;
        // The colours the mode paints with; an invisible run paints nothing.
        if let Some(paint) = [Some("f"), Some("S"), Some("B"), None][state.render as usize] {
            patterns::paint(paint, state.fill_components, state.stroke_components)?;
        }
        // Half the line width, in page units, around stroked glyphs.
        let stroke = if matches!(state.render, 1 | 2) {
            state.line_width / 2.
                * (state.page_transform[0].abs() + state.page_transform[2].abs())
                    .max(state.page_transform[1].abs() + state.page_transform[3].abs())
        } else {
            0.
        };
        object.cursor += advance;
        if !object.cursor.is_finite() || object.cursor > 1_000_000.0 {
            return Err("continued text advance exceeds its limit".into());
        }
        let shown = Shown {
            index,
            name,
            size,
            font_operator,
            metrics,
            text,
            advance,
            horizontal,
            shown_matrix,
            page_matrix,
            read_only,
            stroke,
        };
        let bounds = self.bounds(&shown, state, checked)?;
        let display_rect = self.to_display(bounds);
        if display_rect.iter().any(|v| !v.is_finite()) {
            return Err("text bounds exceed the display range".into());
        }
        self.record(shown, display_rect, state, object, marked)
    }

    /// The show's text, advance and horizontal ink, and whether it backtracks.
    /// A `TJ`'s leading adjustment moves the cursor before the run starts.
    fn measure(
        &mut self,
        index: usize,
        op: &Operation,
        metrics: &fonts::Metrics,
        size: f64,
        state: &Graphics,
        object: &mut TextObject,
    ) -> Result<(String, f64, [f64; 2], bool), String> {
        let mut backtracks = false;
        let (text, advance, horizontal) = if op.operator == "TJ" {
            let (text, advance, horizontal, lead, found, back) = array_text(
                op.operands[0].as_array().map_err(|e| e.to_string())?,
                metrics,
                size,
                state.spacing,
                state.word_spacing,
            )?;
            backtracks = back;
            if found.count > 0 {
                self.gaps
                    .insert(index as u32, found.total / f64::from(found.count));
            }
            if let Some(lead) = lead {
                object.cursor -= number(lead)? * size / 1000.0;
                if !object.cursor.is_finite() || object.cursor.abs() > 1_000_000.0 {
                    return Err("kerning position exceeds its limit".into());
                }
                self.leads.insert(index as u32, lead.clone());
            }
            (text, advance, horizontal)
        } else {
            metrics.source_layout(
                op.operands[0].as_str().map_err(|e| e.to_string())?,
                size,
                state.spacing,
                state.word_spacing,
            )?
        };
        Ok((text, advance, horizontal, backtracks))
    }

    /// The show's hit box in page space: its em envelope, widened to its ink
    /// where that is known and needed, and held to a rectangular clip.
    fn bounds(
        &mut self,
        shown: &Shown,
        state: &Graphics,
        checked: &Checked,
    ) -> Result<[f64; 4], String> {
        let &Shown {
            index,
            name,
            size,
            metrics,
            ref text,
            horizontal,
            page_matrix,
            read_only,
            stroke,
            ..
        } = shown;
        let mut bounds = text_bounds(
            page_matrix,
            [horizontal[0], -size * 0.25, horizontal[1], size],
        );
        if (read_only || metrics.vertical_bounds.is_some()) && !text.is_empty() {
            let (reach, bottom, top) = match (metrics.vertical_bounds, checked.font_boxes.get(name))
            {
                (Some([bottom, top]), _) => (0., bottom, top),
                // A standard font has no outlines here, but its FontBBox holds
                // every glyph: read-only text in it (arXiv's rotated stamp)
                // reserves that box, widened by its larger side at both ends.
                (None, Some(&[left, bottom, right, top])) => {
                    (left.abs().max(right.abs()), bottom, top)
                }
                (None, None) => {
                    return Err("read-only text requires validated glyph outlines".into())
                }
            };
            let reach = reach * size / 1000.;
            let ink = text_bounds(
                page_matrix,
                [
                    horizontal[0].min(horizontal[1]) - reach,
                    bottom * size / 1000.,
                    horizontal[0].max(horizontal[1]) + reach,
                    top * size / 1000.,
                ],
            );
            bounds = [
                bounds[0].min(ink[0]),
                bounds[1].min(ink[1]),
                bounds[2].max(ink[2]),
                bounds[3].max(ink[3]),
            ];
        }
        bounds = [
            bounds[0] - stroke,
            bounds[1] - stroke,
            bounds[2] + stroke,
            bounds[3] + stroke,
        ];
        // Include actual horizontal overhang in hit boxes and clipping. The
        // vertical union covers every offered glyph. Writing additionally keeps
        // replacement ink inside these unrounded original horizontal bounds.
        // Standard-font widths cannot prove substituted glyph ink bounds.
        if !text.is_empty() && (state.clip.is_some() || !state.compound_clips.is_empty()) {
            let [bottom, top] = metrics
                .vertical_bounds
                .ok_or("clipped text requires validated embedded glyph outlines")?;
            let ink_bounds = text_bounds(
                page_matrix,
                [
                    horizontal[0],
                    bottom * size / 1000.,
                    horizontal[1],
                    top * size / 1000.,
                ],
            );
            let ink_bounds = [
                ink_bounds[0] - stroke,
                ink_bounds[1] - stroke,
                ink_bounds[2] + stroke,
                ink_bounds[3] + stroke,
            ];
            // A rectangular clip remains in the saved stream and in the worker
            // preview. Partly clipped source text is still editable; rejecting
            // it here would disable every other text object on the page.
            if clipping::contains(state.clip, ink_bounds).is_err() {
                let rect = state.clip.unwrap();
                bounds = [
                    bounds[0].clamp(rect[0], rect[2]),
                    bounds[1].clamp(rect[1], rect[3]),
                    bounds[2].clamp(rect[0], rect[2]),
                    bounds[3].clamp(rect[1], rect[3]),
                ];
            }
            for region in &state.compound_clips {
                region.contains(ink_bounds)?;
            }
            if !state.compound_clips.is_empty() {
                self.compound_run_clips
                    .insert(index as u32, (state.compound_clips.clone(), ink_bounds));
            }
        }
        Ok(bounds)
    }

    /// Files the measured show where it belongs: under its structure Span,
    /// with an open spacer or ActualText span, and as a read-only or an
    /// editable run.
    fn record(
        &mut self,
        shown: Shown,
        display_rect: [f32; 4],
        state: &Graphics,
        object: &TextObject,
        marked: &mut Marked,
    ) -> Result<(), String> {
        let Shown {
            index,
            name,
            size,
            font_operator,
            metrics,
            text,
            advance,
            horizontal,
            shown_matrix,
            page_matrix,
            read_only,
            stroke,
        } = shown;
        if let Some(span) = marked.tags.actual() {
            let (shows, plain) = self.span_shows.entry(span).or_insert((Vec::new(), true));
            shows.push(index as u32);
            *plain &= marked.actual.is_none();
        }
        if let Some(spacer) = &marked.spacer {
            spacer.text(&text)?;
            return Ok(());
        }
        if let Some(span) = &mut marked.actual {
            span.show(index as u32, &text, metrics.vertical_bounds.is_some())?;
        }
        if read_only {
            self.preserved.push(Run {
                display_rect,
                minimum_height: None,
                operator: index as u32,
                text,
                font: String::from_utf8_lossy(name).into_owned(),
                size,
                matrix: page_matrix,
                advance,
            });
            return Ok(());
        }
        self.font_operators.insert(index as u32, font_operator);
        self.horizontal_bounds.insert(index as u32, horizontal);
        self.text_spacing
            .insert(index as u32, (state.spacing, state.word_spacing));
        self.contexts.insert(
            index as u32,
            layout::Context {
                line_origin: object.line_origin,
                shown: shown_matrix,
                cursor_after: object.cursor,
                clip: state.clip,
                regions: state.compound_clips.clone(),
                stroke,
                size,
                scale: page_matrix[0].hypot(page_matrix[1]),
                transform: state.page_transform,
            },
        );
        if marked.tags.centred() {
            self.centred.insert(index as u32);
        }
        self.runs.runs.push(Run {
            display_rect,
            minimum_height: metrics
                .vertical_bounds
                .filter(|bounds| bounds[0] < -250.)
                .map(|bounds| {
                    size * page_matrix[2].hypot(page_matrix[3]) * (1. - bounds[0] / 1000.)
                }),
            operator: index as u32,
            text,
            font: String::from_utf8_lossy(name).into_owned(),
            size,
            matrix: page_matrix,
            advance,
        });
        Ok(())
    }
}

/// Whether the marked-content sequence opened at `index` closes at once,
/// with no operator inside it.
fn closes(content: &Content, index: usize) -> bool {
    content
        .operations
        .get(index + 1)
        .is_some_and(|next| next.operator == "EMC" && next.operands.is_empty())
}

/// Which Spans carry their one run's own text as ActualText, and which do
/// not and are scanned again read-only (`inspect`).
pub(super) fn settle_spans(
    doc: &Document,
    inspection: &mut Inspection,
    span_shows: BTreeMap<ObjectId, (Vec<u32>, bool)>,
) {
    // A Span's ActualText is rewritten with its run only where it is that
    // run's text: every show of the Span plain text in one run, and nothing
    // else in the run, with the texts equal but for spaces at either end.
    for (span, (shows, plain)) in span_shows {
        let owner = |run: &Run| {
            std::iter::once(run.operator)
                .chain(
                    inspection
                        .groups
                        .get(&run.operator)
                        .into_iter()
                        .flatten()
                        .copied(),
                )
                .collect::<BTreeSet<_>>()
        };
        let touching: Vec<&Run> = inspection
            .runs
            .runs
            .iter()
            .filter(|run| owner(run).iter().any(|show| shows.contains(show)))
            .collect();
        let element = match touching.as_slice() {
            [run] if plain && shows.iter().all(|show| owner(run).contains(show)) => doc
                .get_dictionary(span)
                .ok()
                .and_then(|dict| dict.get(b"ActualText").ok())
                .and_then(|value| actual::logical(value).ok())
                .and_then(|text| actual::Edges::of(&text, &run.text))
                .map(|edges| (run.operator, actual::Element { span, edges })),
            _ => None,
        };
        match element {
            Some((operator, element)) => {
                inspection.structure_actual.insert(operator, element);
            }
            None => {
                inspection.demote.insert(span);
            }
        }
    }
}
