use std::fs;

// Include CLI definitions directly — build scripts cannot depend on the crate
// they build. IMPORTANT: cli.rs must only use std + clap imports (no `use crate::...`).
include!("src/cli.rs");

fn main() {
    use clap::CommandFactory;

    let out_dir =
        std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR not set by cargo"));
    let man_dir = out_dir.join("man");
    fs::create_dir_all(&man_dir).expect("creating man output dir");

    let cmd = Cli::command();

    // Main man page: code-primer(1)
    let man = clap_mangen::Man::new(cmd.clone());
    let mut buf = Vec::new();
    man.render(&mut buf).expect("rendering man page");
    fs::write(man_dir.join("code-primer.1"), buf).expect("writing code-primer.1");

    // Subcommand man pages: code-primer-init(1), code-primer-generate(1), etc.
    for subcmd in cmd.get_subcommands() {
        if subcmd.get_name() == "help" {
            continue;
        }
        let name: &'static str = Box::leak(
            format!("code-primer-{}", subcmd.get_name()).into_boxed_str(),
        );
        let man = clap_mangen::Man::new(subcmd.clone().name(name));
        let mut buf = Vec::new();
        man.render(&mut buf).expect("rendering subcommand man page");
        fs::write(man_dir.join(format!("{name}.1")), buf)
            .expect("writing subcommand man page");
    }

    println!("cargo:rerun-if-changed=src/cli.rs");
}
