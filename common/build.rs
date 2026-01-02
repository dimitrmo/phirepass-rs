use std::io::Result;

fn main() -> Result<()> {
    // Create output directory if it doesn't exist
    std::fs::create_dir_all("src/protocol/generated")?;

    prost_build::Config::new()
        .out_dir("src/protocol/generated")
        .compile_protos(
            &[
                "proto/common.proto",
                "proto/sftp.proto",
                "proto/web.proto",
                "proto/node.proto",
                "proto/frame.proto",
            ],
            &["proto/"],
        )?;

    Ok(())
}
