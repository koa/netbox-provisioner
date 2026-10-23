use std::{env, fs, io::Write, path::Path, time::SystemTime};

/// The frontend is built before the binary (`build.sh`, `local/lab/run-provisioner.sh`), not
/// from here: a build script can't wait for `trunk` without blocking cargo's lock, and one that
/// doesn't wait embeds whatever half of `dist` is there.
const DIST: &str = "../frontend/dist";

fn main() -> std::io::Result<()> {
    println!("cargo:rerun-if-changed={DIST}");
    if !Path::new(DIST).join("index.html").exists() {
        panic!(
            "{DIST}/index.html is missing: build the frontend first \
             (cd frontend/node && npm install; cd .. && trunk build --release)"
        );
    }
    // the embedded files' modification time: when `dist` changed last
    let epoch_time = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let outdir = env::var("OUT_DIR").unwrap();
    let outfile = format!("{}/timestamp.txt", outdir);

    let mut fh = fs::File::create(&outfile).unwrap();
    write!(fh, "{}", epoch_time)?;
    Ok(())
}
