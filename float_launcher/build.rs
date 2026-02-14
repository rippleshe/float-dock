use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=ico/app.ico");
    println!("cargo:rerun-if-changed=ico/favicon.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let icon = ["ico/app.ico", "ico/favicon.ico"]
        .into_iter()
        .find(|p| Path::new(p).is_file());

    let Some(icon_path) = icon else {
        println!(
            "cargo:warning=No icon found at ico/app.ico or ico/favicon.ico; exe icon resource not set"
        );
        return;
    };

    let mut res = winres::WindowsResource::new();
    res.set_icon(icon_path);
    res.set("ProductName", "Float Dock");
    res.set("FileDescription", "Float Dock");
    res.set("OriginalFilename", "float_dock.exe");
    res.set("InternalName", "float_dock");
    if let Err(err) = res.compile() {
        panic!("failed to compile Windows resource icon from {icon_path}: {err}");
    }
}
