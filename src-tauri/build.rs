fn main() {
    println!("cargo:rerun-if-env-changed=OSDM_UPDATE_ENDPOINT");
    println!("cargo:rerun-if-env-changed=OSDM_UPDATE_PUBKEY");
    println!("cargo:rerun-if-env-changed=OSDM_CHROME_EXTENSION_ID");
    println!("cargo:rerun-if-env-changed=OSDM_EDGE_EXTENSION_ID");
    println!("cargo:rerun-if-env-changed=OSDM_FIREFOX_ADDON_SLUG");
    tauri_build::build();
}
