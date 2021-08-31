extern crate cmake;
extern crate cxx_build;

use std::env;
use std::io;
use std::path::PathBuf;

fn main() -> io::Result<()> {
    let vcpkg_root = env::var("VCPKG_ROOT").expect("must set VCPKG_ROOT");

    let gqlmapi = cmake::Config::new("gqlmapi")
        .define(
            "CMAKE_TOOLCHAIN_FILE",
            format!("{}/scripts/buildsystems/vcpkg.cmake", vcpkg_root),
        )
        .define("BUILD_TESTING", "OFF")
        .cxxflag("/EHsc")
        .profile("RelWithDebInfo")
        .build();

    println!("cargo:rustc-link-search=native={}/lib", gqlmapi.display());
    println!("cargo:rustc-link-search=native={}/bin", gqlmapi.display());

    let vcpkg_triplet = env::var("VCPKG_TARGET_TRIPLET").expect("must set VCPKG_TARGET_TRIPLET");
    let mut vcpkg_installed = PathBuf::from(vcpkg_root);
    vcpkg_installed.push("installed");
    vcpkg_installed.push(vcpkg_triplet);

    println!(
        "cargo:rustc-link-search=native={}/lib",
        vcpkg_installed.display()
    );

    let cpp_dlls = [
        "gqlmapi",
        "graphqlservice",
        "graphqlintrospection",
        "graphqlpeg",
        "graphqlresponse",
        "graphqljson",
    ];
    for dll in cpp_dlls {
        println!("cargo:rustc-link-lib=dylib={}", dll);
    }

    let mut gqlmapi_include = gqlmapi;
    gqlmapi_include.push("include");
    let mut vcpkg_include = vcpkg_installed.clone();
    vcpkg_include.push("include");

    cxx_build::bridge("src/bindings.rs")
        .file("src/Bindings.cpp")
        .include(gqlmapi_include)
        .include(vcpkg_include)
        .flag_if_supported("/std:c++17")
        .flag_if_supported("/EHsc")
        .compile("gqlmapi_rs");

    Ok(())
}
