fn main() {
    // Embed icon as Windows resource (shows in taskbar/explorer).
    #[cfg(target_os = "windows")]
    {
        embed_resource::compile("el15.rc", embed_resource::NONE);
    }
}
