use std::path::Path;
use std::process::Command;

fn main() {
    // Re-run if dashboard source files change.
    println!("cargo:rerun-if-changed=../../dashboard/src");
    println!("cargo:rerun-if-changed=../../dashboard/server");
    println!("cargo:rerun-if-changed=../../dashboard/package.json");
    println!("cargo:rerun-if-changed=../../dashboard/build.mjs");

    let dashboard = Path::new("../../dashboard");
    if !dashboard.exists() {
        println!("cargo:warning=dashboard/ directory not found, skipping build");
        return;
    }

    let dist_server = dashboard.join("dist-server/index.cjs");

    // Install node_modules if needed.
    if !dashboard.join("node_modules").exists() {
        println!("cargo:warning=dashboard: running npm install");
        let status = Command::new("npm")
            .args(["install", "--legacy-peer-deps"])
            .current_dir(dashboard)
            .status();
        match status {
            Ok(s) if s.success() => {}
            Ok(s) => {
                println!("cargo:warning=dashboard: npm install exited with {s}");
                return;
            }
            Err(e) => {
                println!("cargo:warning=dashboard: npm install failed: {e}");
                return;
            }
        }
    }

    // Build if dist-server/index.cjs is missing.
    if !dist_server.exists() {
        println!("cargo:warning=dashboard: running npm run build:fast");
        let status = Command::new("npm")
            .args(["run", "build:fast"])
            .current_dir(dashboard)
            .status();
        match status {
            Ok(s) if s.success() => {
                println!("cargo:warning=dashboard: build complete");
            }
            Ok(s) => println!("cargo:warning=dashboard: build exited with {s}"),
            Err(e) => println!("cargo:warning=dashboard: build failed: {e}"),
        }
    }
}
