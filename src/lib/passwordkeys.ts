/** Keep keyboard commands and sequential focus inside a password prompt. */
export function passwordKeys(
  dialog: HTMLDialogElement, fields: HTMLInputElement[], lastButton: HTMLButtonElement,
  dismiss: () => void, submit: () => void,
): void {
  dialog.addEventListener("cancel", (event) => {
    event.preventDefault();
    dismiss();
  });
  dialog.addEventListener("keydown", (event) => {
    event.stopPropagation();
    if (event.isComposing) return;
    const first = fields[0]!;
    if (event.key === "Escape") {
      event.preventDefault();
      dismiss();
    } else if (event.key === "Enter" && fields.some((field) => event.target === field)) {
      event.preventDefault();
      submit();
    } else if (event.key === "Tab") {
      if (event.shiftKey && event.target === first) {
        event.preventDefault();
        lastButton.focus();
      } else if (!event.shiftKey && event.target === lastButton) {
        event.preventDefault();
        first.focus();
      }
    } else if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === "f") {
      event.preventDefault();
    }
  });
}
