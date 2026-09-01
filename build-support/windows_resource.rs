//! Shared Windows PE version-resource wiring for installer-owned binaries.

/// Apply the package build version to both the numeric PE resource and the
/// user-visible version strings. Windows Installer compares the numeric file
/// version when a late major upgrade must replace the maintenance executable
/// before removing the related product.
pub fn apply_package_version(resource: &mut winresource::WindowsResource, file_type: u64) {
    println!("cargo:rerun-if-env-changed=PICOO_WINDOWS_FILE_VERSION");

    let package_version = std::env::var("PICOO_WINDOWS_FILE_VERSION")
        .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string());
    let fields = package_version
        .split('.')
        .map(|field| {
            field.parse::<u16>().unwrap_or_else(|_| {
                panic!(
                    "PICOO_WINDOWS_FILE_VERSION must contain three u16 fields: {package_version}"
                )
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        fields.len(),
        3,
        "PICOO_WINDOWS_FILE_VERSION must contain three numeric fields: {package_version}"
    );

    let numeric =
        (u64::from(fields[0]) << 48) | (u64::from(fields[1]) << 32) | (u64::from(fields[2]) << 16);
    let display = format!("{package_version}.0");
    resource
        .set_version_info(winresource::VersionInfo::FILEVERSION, numeric)
        .set_version_info(winresource::VersionInfo::PRODUCTVERSION, numeric)
        .set_version_info(winresource::VersionInfo::FILETYPE, file_type)
        .set("FileVersion", &display)
        .set("ProductVersion", &display);
}
