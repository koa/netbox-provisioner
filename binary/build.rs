use std::{env, fs, io::Write, time::SystemTime};
fn main() -> std::io::Result<()> {
    std::process::Command::new("trunk")
        .current_dir("../frontend/")
        .args(["build", "--release"])
        .spawn()?;
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
