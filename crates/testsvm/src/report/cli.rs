//! The ready-made report CLI every consumer would otherwise hand-write: the
//! `--check` / `--explain` / default-write dispatch over a configured
//! [`Reporter`]. The consumer builds and pre-configures the `Reporter` (so it can
//! register normalize overrides or set the group order first) and supplies its
//! own heading/intro; the three modes, written and tested once here, are shared
//! across clients. A consumer needing a bespoke mode drops to the `Reporter`
//! methods directly instead of calling this.

use {
    crate::report::{render_explain, Manifest, Reporter},
    std::path::Path,
};

/// Run the standard report CLI against a configured `reporter`, writing into
/// `out_dir` with the given `heading`/`intro`. The mode is read from the process
/// arguments:
///
/// - `--check`: diff the fresh run against the committed `<out_dir>/fingerprint.txt`;
///   print `fingerprint OK`, or the changed tests (with captured JSON) and exit 1.
/// - `--explain`: diff the fresh run against the committed `<out_dir>/baseline.tar.gz`,
///   leading with `Changed`.
/// - default: write `index.md` + `fingerprint.txt` + `baseline.tar.gz` into `out_dir`.
pub fn run_cli(reporter: &Reporter, out_dir: &str, heading: &str, intro: &str) {
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--check") {
        let committed = std::fs::read_to_string(format!("{out_dir}/fingerprint.txt"))
            .expect("committed fingerprint.txt (run report once and commit it)");
        let changes = reporter.verify(&Manifest::parse(&committed));
        if changes.is_empty() {
            println!("fingerprint OK");
        } else {
            print!("{}", reporter.render_changes(&changes));
            std::process::exit(1);
        }
        return;
    }

    if args.iter().any(|a| a == "--explain") {
        match reporter.explain(Path::new(out_dir)) {
            Some(diffs) => print!("{}", render_explain(&diffs)),
            None => println!(
                "explain: no committed baseline (run report once and commit {out_dir}/baseline.tar.gz)"
            ),
        }
        return;
    }

    reporter.write(out_dir, heading, intro);
    reporter.write_baseline(Path::new(out_dir));
    println!("wrote index.md + fingerprint.txt + baseline.tar.gz to {out_dir}");
}
