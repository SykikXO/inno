# Maintainer: Sykik [xo.sykik@gmail.com]
pkgname=inno
pkgver=0.2.0
pkgrel=1
pkgdesc="A lightweight, event-driven Wayland notification agent (Rust)"
arch=('x86_64')
url="https://github.com/SykikXO/inno"
license=('MIT')
depends=('wayland' 'cairo' 'dbus' 'glibc')
makedepends=('rust' 'cargo')
backup=('etc/xdg/inno/inno.conf')
source=()
md5sums=()

build() {
  cd "$srcdir/.."
  cargo build --release
}

package() {
  cd "$srcdir/.."
  install -Dm755 target/release/inno "$pkgdir/usr/bin/inno"
  install -Dm644 inno.conf "$pkgdir/etc/xdg/inno/inno.conf"
  install -Dm644 inno.service "$pkgdir/usr/lib/systemd/user/inno.service"
}
