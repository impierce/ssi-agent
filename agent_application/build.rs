use std::env;
use std::fs::File;
use std::io;
use std::io::BufWriter;
use std::io::Write;

fn main() -> io::Result<()> {
    let file = File::create("./src/metadata/values.rs")?;
    // let dest_path = Path::new("./src/metadata").join("values.rs");

    let mut writer = BufWriter::new(file);

    let app_version = env::var("APP_VERSION").ok();
    let git_commit_hash = env::var("GIT_COMMIT_HASH").ok();
    let docker_build_timestamp = env::var("DOCKER_BUILD_TIMESTAMP").ok();

    writeln!(writer, "// THIS FILE IS OVERWRITTEN BY A BUILD SCRIPT")?;
    writeln!(writer, "pub const APP_VERSION: Option<&str> = {:?};", app_version)?;
    writeln!(
        writer,
        "pub const GIT_COMMIT_HASH: Option<&str> = {:?};",
        git_commit_hash
    )?;
    writeln!(
        writer,
        "pub const DOCKER_BUILD_TIMESTAMP: Option<&str> = {:?};",
        docker_build_timestamp
    )?;

    writer.flush()?;

    Ok(())
}
