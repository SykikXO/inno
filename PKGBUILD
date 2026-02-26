# Maintainer: Sykik [xo.sykik@gmail.com]
pkgname=inno
pkgver=0.4.0
pkgrel=1
pkgdesc="A lightweight, event-driven Wayland notification agent (Rust)"
arch=('x86_64')
url="https://github.com/SykikXO/inno"
license=('MIT')
depends=('wayland' 'cairo' 'dbus' 'glibc' 'ttf-jetbrains-mono-nerd')
makedepends=('rust' 'cargo')
backup=('etc/xdg/inno/inno.toml'
        'etc/xdg/inno/events/battery.toml'
        'etc/xdg/inno/events/bluetooth.toml'
        'etc/xdg/inno/events/laptop_battery.toml'
        'etc/xdg/inno/events/headset_battery.toml')
install=inno.install
source=()
md5sums=()

build() {
  cd "$srcdir/.."
  cargo build --release
}

package() {
  cd "$srcdir/.."
  install -Dm755 target/release/inno "$pkgdir/usr/bin/inno"
  install -Dm644 inno.toml "$pkgdir/etc/xdg/inno/inno.toml"
  for f in events/*.toml; do
    install -Dm644 "$f" "$pkgdir/etc/xdg/inno/$f"
  done
}
