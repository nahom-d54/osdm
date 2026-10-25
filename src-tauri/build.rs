fn main() {
    println!("cargo:rerun-if-env-changed=OSDM_UPDATE_ENDPOINT");
    println!("cargo:rerun-if-env-changed=OSDM_UPDATE_PUBKEY");
    tauri_build::build();
}
