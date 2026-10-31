pub fn restore_sigpipe() {
    // SAFETY: SIG_DFL installs no handler, so no handler can run asynchronously against this process.
    unsafe { libc::signal(libc::SIGPIPE, libc::SIG_DFL) };
}

pub(crate) fn privileged() -> bool {
    // SAFETY: geteuid takes no argument and cannot fail, so the block has no precondition to keep.
    (unsafe { libc::geteuid() }) == 0
}
