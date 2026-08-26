import { copyFile, mkdir } from "node:fs/promises";
import { existsSync } from "node:fs";
import { spawnSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const scriptDirectory = path.dirname(fileURLToPath(import.meta.url));
const extensionRoot = path.resolve(scriptDirectory, "..");
const nativeRoot = path.resolve(extensionRoot, "..", "native", "CodexLidGuard");
const manifest = path.join(nativeRoot, "Cargo.toml");
const cargoFromHome = process.env.USERPROFILE
  ? path.join(process.env.USERPROFILE, ".cargo", "bin", "cargo.exe")
  : undefined;
const cargo = process.env.CARGO
  || (cargoFromHome && existsSync(cargoFromHome) ? cargoFromHome : "cargo");

const build = spawnSync(cargo, ["build", "--release", "--locked", "--manifest-path", manifest], {
  cwd: nativeRoot,
  stdio: "inherit",
  windowsHide: true
});
if (build.error) {
  throw build.error;
}
if (build.status !== 0) {
  throw new Error(`Rust helper build failed with exit code ${build.status}`);
}

const outputRoot = path.join(extensionRoot, "bin", "win-x64");
const soundsRoot = path.join(outputRoot, "sounds");
await mkdir(soundsRoot, { recursive: true });
await Promise.all([
  copyFile(path.join(nativeRoot, "target", "release", "CodexLidGuard.exe"), path.join(outputRoot, "CodexLidGuard.exe")),
  copyFile(path.join(nativeRoot, "Assets", "Herdr", "done.mp3"), path.join(soundsRoot, "done.mp3")),
  copyFile(path.join(nativeRoot, "Assets", "Herdr", "request.mp3"), path.join(soundsRoot, "request.mp3")),
  copyFile(path.join(nativeRoot, "Assets", "Herdr", "LICENSE.txt"), path.join(soundsRoot, "HERDR-LICENSE.txt"))
]);
