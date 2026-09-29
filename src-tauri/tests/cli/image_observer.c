// Test-only dyld observer. Build as a dylib with clang -dynamiclib, then inject
// with DYLD_INSERT_LIBRARIES and TPDF_IMAGE_LOG_DIR naming a fresh directory.
// One file per PID avoids dyld's non-atomic shared-stderr prefixes. Nothing in
// the application supplies these observations. Never ship this library.
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <mach-o/dyld.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static int log_fd = -1;

static void record(const char *line) {
    size_t left = strlen(line);
    while (left) {
        ssize_t n = write(log_fd, line, left);
        if (n < 0 && errno == EINTR) continue;
        if (n <= 0) _exit(125);
        line += n;
        left -= (size_t)n;
    }
}

static void loaded(const struct mach_header *header, intptr_t slide) {
    (void)slide;
    Dl_info info;
    if (!dladdr(header, &info) || !info.dli_fname) _exit(125);
    record(info.dli_fname);
    record("\n");
}

static void finished(void) { record("END\n"); }

__attribute__((constructor)) static void observe(void) {
    const char *dir = getenv("TPDF_IMAGE_LOG_DIR");
    char path[PATH_MAX];
    if (!dir || snprintf(path, sizeof(path), "%s/%d.images", dir, getpid()) >= (int)sizeof(path))
        _exit(125);
    int fd = open(path, O_WRONLY | O_CREAT | O_EXCL, 0600);
    if (fd < 0) _exit(125);
    // The worker owns low descriptor numbers. Never keep one there, and never
    // inherit our log into another exec: its constructor opens its own file.
    log_fd = fcntl(fd, F_DUPFD_CLOEXEC, 64);
    close(fd);
    if (log_fd < 0) _exit(125);
    _dyld_register_func_for_add_image(loaded);
    record("READY\n");
    if (atexit(finished) != 0) _exit(125);
}
