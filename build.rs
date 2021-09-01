extern crate cmake;
extern crate cxx_build;

use std::env;
use std::io;
use std::path::PathBuf;

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-env-changed=VCPKG_ROOT");

    let vcpkg_root = env::var("VCPKG_ROOT").expect("must set VCPKG_ROOT");

    let platform = if cfg!(target_pointer_width = "64") { "x64-windows" } else { "x86-windows" };
    let vcpkg_static = cfg!(feature = "crt-static");
    let vcpkg_triplet = if vcpkg_static { format!("{}-static", platform) } else { String::from(platform) };

    let gqlmapi = cmake::Config::new("gqlmapi")
        .define(
            "CMAKE_TOOLCHAIN_FILE",
            format!("{}/scripts/buildsystems/vcpkg.cmake", vcpkg_root),
        )
        .define("VCPKG_TARGET_TRIPLET", &vcpkg_triplet)
        .define("BUILD_SHARED_LIBS", if vcpkg_static { "OFF" } else { "ON" })
        .define("BUILD_TESTING", "OFF")
        .cxxflag("/EHsc")
        .profile("RelWithDebInfo")
        .build();

    println!("cargo:rustc-link-search=native={}/lib", gqlmapi.display());

    if !vcpkg_static {
        println!("cargo:rustc-link-search=native={}/bin", gqlmapi.display());
    }

    let mut vcpkg_installed = PathBuf::from(vcpkg_root);
    vcpkg_installed.push("installed");
    vcpkg_installed.push(vcpkg_triplet);

    println!(
        "cargo:rustc-link-search=native={}/lib",
        vcpkg_installed.display()
    );

    if vcpkg_static {
        let cpp_libs = [
            "gqlmapi",
            "gqlmapiCommon",
            "mapischema",
            "mapistub",
            "graphqlservice",
            "graphqlintrospection",
            "graphqlpeg",
            "graphqlresponse",
            "graphqljson",
        ];

        for lib in cpp_libs {
            println!("cargo:rustc-link-lib=static={}", lib);
        }
    } else {
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
        .static_crt(vcpkg_static)
        .compile("gqlmapi-rs");

    Ok(())
}
