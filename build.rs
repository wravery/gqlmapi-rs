extern crate cmake;
extern crate cxx_build;

use std::{
    env,
    fs::File,
    io::{self, Read},
    path::PathBuf,
};

fn main() -> io::Result<()> {
    println!("cargo:rerun-if-env-changed=VCPKG_ROOT");
    let vcpkg_root = env::var("VCPKG_ROOT").map_or_else(
        |_| {
            // Try to find %LOCALAPPDATA%\vcpkg\vcpkg.path.txt if %VCPKG_ROOT% was not set.
            println!("cargo:rerun-if-env-changed=LOCALAPPDATA");
            let mut vcpkg_app_data = PathBuf::from(env!("LOCALAPPDATA"));
            vcpkg_app_data.push("vcpkg");
            vcpkg_app_data.push("vcpkg.path.txt");
            println!("cargo:rerun-if-changed={}", vcpkg_app_data.display());
            let mut vcpkg_path_txt = File::open(&vcpkg_app_data)
                .expect(format!("Failed to open: {}", vcpkg_app_data.display()).as_str());
            let mut buf = Vec::new();
            vcpkg_path_txt
                .read_to_end(&mut buf)
                .expect(format!("Failed to read: {}", vcpkg_app_data.display()).as_str());
            String::from_utf8(buf)
                .expect(format!("Failed to decode: {}", vcpkg_app_data.display()).as_str())
        },
        |value| value,
    );

    let platform = if cfg!(target_pointer_width = "64") {
        "x64-windows"
    } else {
        "x86-windows"
    };
    let vcpkg_static = cfg!(target_feature = "crt-static");
    let vcpkg_triplet = if vcpkg_static {
        format!("{}-static", platform)
    } else {
        String::from(platform)
    };

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
