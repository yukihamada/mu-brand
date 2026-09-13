"""Run the exact catalog handler + contract test in a small offline Rust crate.

No reconstructed catalog, mocked Axum serialization, server main or cron startup.
Only test build artifacts are written under the approved temp directory.
"""
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "store/src/catalog.rs"
TEMP = Path("/var/folders/5h/3zhkzzk12_1g06p6w2vxc45r0000gn/T/sente")
TARGET = TEMP / "mu-ios-contract-check"
text = SOURCE.read_text()


def section(start, end):
    a = text.index(start)
    return text[a:text.index(end, a)]


parts = [
    "#![allow(dead_code)]\nuse axum::{http::StatusCode, response::{Response, IntoResponse}};",
    section("pub(crate) fn route_for_kind(", "// ─── Manufacturing Router"),
    section("struct ProductSpec {", "/// Public, agent-facing view of a `ProductSpec`"),
    section("pub const MAKE_KINDS_ALL:", "#[cfg(test)]\nmod make_kinds_tests"),
    "#[cfg(test)] mod make_kinds_tests { use super::*;\n" + section(
        "    #[tokio::test]\n    async fn make_kinds_matches_ios_contract_fixture()",
        "    async fn catalog_items()",
    ) + "\n}",
]
# Resolve the production test's manifest-relative fixture path in this extracted crate.
code = "\n".join(parts).replace('env!("CARGO_MANIFEST_DIR")', '"' + str(ROOT / "store") + '"')
TARGET.mkdir(exist_ok=True)
(TARGET / "src").mkdir(exist_ok=True)
(TARGET / "src/lib.rs").write_text(code)
(TARGET / "Cargo.toml").write_text('''[package]
name = "mu-ios-contract-check"
version = "0.0.0"
edition = "2021"
[dependencies]
axum = { version = "0.7", features = ["json"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
''')
subprocess.run([
    "cargo", "test", "--offline", "--jobs", "2", "--manifest-path", str(TARGET / "Cargo.toml"),
    "make_kinds_tests::make_kinds_matches_ios_contract_fixture", "--", "--exact",
], check=True)
