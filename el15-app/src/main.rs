// On Windows, hide the console window when running in GUI mode.
// The console is still attached if the app is launched from a terminal.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

mod cli;
mod cli_run;
mod graph;
mod gui;
mod hid_flash;
mod i18n;
mod logging;
mod settings;
mod usb;

// `rust_i18n::i18n!` MUST be invoked at the crate root so that the generated
// helper symbol `_rust_i18n_t` is available to callers of `rust_i18n::t!`.
rust_i18n::i18n!("locales", fallback = "en");

use clap::Parser;

fn main() -> anyhow::Result<()> {
    let args = cli::Cli::parse();

    // On Windows with `windows_subsystem = "windows"`, re-attach to the parent
    // console when running in CLI mode so output is visible.
    #[cfg(target_os = "windows")]
    if args.no_gui || args.list_usb || args.scan || args.flash.is_some() || args.debug {
        unsafe { winapi::um::wincon::AttachConsole(winapi::um::wincon::ATTACH_PARENT_PROCESS); }
    }

    logging::init(args.verbose, args.log.as_deref(), args.verbose_ble, args.verbose_gui)?;

    if args.no_gui || args.list_usb || args.scan || args.flash.is_some() || args.debug {
        // CLI / headless mode: handle one-shot subcommands or run SCPI server.
        let rt = tokio::runtime::Runtime::new()?;
        rt.block_on(cli_run::run(args))
    } else {
        gui::run(args)
    }
}
