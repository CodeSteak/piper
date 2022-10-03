#! /usr/bin/env ruby

rel = 0
pkgname = File.read("Cargo.toml").scan(/^name\s*=\s*\"([\w-]+)\"/)[0][0]
version = File.read("Cargo.toml").scan(/^version\s*=\s*\"([\d\.]+)\"/)[0][0]

puts "#{pkgname} #{version}"
Dir.mkdir("archpkg") if !Dir.exists?("archpkg")

system("tar", "-czvf", "archpkg/#{pkgname}-#{version}.tar.gz",  "src", "Cargo.toml", "templates", "static")

conf = %{
[general]
hostname = "localhost:8000"
listen = "[::1]:8000"
}
File.write("archpkg/#{pkgname}.toml", conf)

require 'digest'

service = %{
[Unit]
Description=Project Home
Requires=network-online.target
After=network-online.target

[Service]
DynamicUser=yes
ProtectSystem=full
PrivateDevices=true
NoNewPrivileges=true

Type=simple
Restart=on-failure
RestartSec=30

WorkingDirectory=/usr/share/#{pkgname}/
Environment=CONFIG_FILE=/usr/share/#{pkgname}/#{pkgname}.toml
ExecStart=/usr/bin/#{pkgname}

[Install]
WantedBy=multi-user.target
}
File.write("archpkg/#{pkgname}.service", service)

pkgbuild = %{
pkgname=#{pkgname}
pkgver=#{version}
pkgrel=#{rel}
pkgdesc="Tar Cloud"
depends=('gcc-libs')
makedepends=('cargo')
arch=('x86_64')
license=('MIT')

source=("#{pkgname}-#{version}.tar.gz"
        "#{pkgname}.service"
        "#{pkgname}.toml")

sha256sums=('#{Digest::SHA2.new(256).hexdigest File.read("archpkg/#{pkgname}-#{version}.tar.gz")}'
            '#{Digest::SHA2.new(256).hexdigest service}'
            '#{Digest::SHA2.new(256).hexdigest conf}')

backup=('usr/share/#{pkgname}/#{pkgname}.toml')

build() {
  cargo build --release
}

check() {
  true
}

package() {
  install -Dm644 "#{pkgname}.toml" "$pkgdir"/usr/share/#{pkgname}/#{pkgname}.toml
  install -Dm644 "#{pkgname}.service" "$pkgdir"/usr/lib/systemd/system/#{pkgname}.service

  find static/ -type f -exec install -Dm644 {} "$pkgdir"/usr/share/#{pkgname}/{} \\;

  install -Dm755 "target/release/#{pkgname}" "$pkgdir/usr/bin/#{pkgname}"
}
}
File.write("archpkg/PKGBUILD", pkgbuild)

Dir.chdir "archpkg/"
system("makepkg", "-f")
