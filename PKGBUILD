# Maintainer: Revaz G <goguadze.revaz@kiu.edu.ge>
pkgname=annotate-linux-git
pkgver=0.1.0
pkgrel=1
pkgdesc="Keyboard-driven screen annotation overlay for wlr-layer-shell compositors (Hyprland, Sway, river)"
arch=('x86_64' 'aarch64')
url="https://github.com/Revaz-Goguadze/annotate-linux"
license=('MIT')
depends=('cairo' 'libxkbcommon' 'wayland')
makedepends=('cargo' 'git')
provides=('annotate-linux')
conflicts=('annotate-linux')
source=("git+$url.git")
sha256sums=('SKIP')

pkgver() {
  cd annotate-linux
  printf "%s.r%s.%s" "0.1.0" "$(git rev-list --count HEAD)" "$(git rev-parse --short HEAD)"
}

build() {
  cd annotate-linux
  export RUSTUP_TOOLCHAIN=stable
  export CARGO_TARGET_DIR=target
  cargo build --release --locked
}

check() {
  cd annotate-linux
  cargo test --release --locked
}

package() {
  cd annotate-linux
  install -Dm755 "target/release/annotate-linux" "$pkgdir/usr/bin/annotate-linux"
  install -Dm644 contrib/hyprland.conf.example "$pkgdir/usr/share/doc/annotate-linux/hyprland.conf.example"
  install -Dm644 contrib/config.example.toml "$pkgdir/usr/share/doc/annotate-linux/config.example.toml"
  install -Dm644 README.md "$pkgdir/usr/share/doc/annotate-linux/README.md"
}
