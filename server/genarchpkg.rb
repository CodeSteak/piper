#! /usr/bin/env ruby

rel =  Time.now.to_i / 100
pkgname = File.read("Cargo.toml").scan(/^name\s*=\s*\"([\w-]+)\"/)[0][0]
version = File.read("Cargo.toml").scan(/^version\s*=\s*\"([\d\.]+)\"/)[0][0]

puts "#{pkgname} #{version}"
Dir.mkdir("archpkg") if !Dir.exists?("archpkg")

system("tar", "-czvf", "archpkg/#{pkgname}-#{version}.tar.gz",  "src", "Cargo.toml", "templates", "static", "tiny-http/Cargo.toml", "tiny-http/src")

conf = %{
[general]
hostname = "localhost:8000"
listen = "[::1]:8000"
}
File.write("archpkg/#{pkgname}.toml", conf)

require 'digest'

sysusers = %{
u tarcloud 248 "tarcloud user" /var/lib/tarcloud /bin/bash
}
File.write("archpkg/#{pkgname}.sysusers", sysusers)

service = %{
[Unit]
Description=Tar Cloud
Requires=network-online.target
After=network-online.target

[Service]
User=tarcloud
ProtectSystem=full
PrivateDevices=yes
PrivateTmp=yes
NoNewPrivileges=true

Type=simple
Restart=on-failure
RestartSec=30

ReadWritePaths=/var/lib/#{pkgname}/
WorkingDirectory=/var/lib/#{pkgname}/
Environment=CONFIG_FILE=/etc/#{pkgname}.toml
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
        "#{pkgname}.sysusers"
        "#{pkgname}.toml")

sha256sums=('#{Digest::SHA2.new(256).hexdigest File.read("archpkg/#{pkgname}-#{version}.tar.gz")}'
            '#{Digest::SHA2.new(256).hexdigest service}'
            '#{Digest::SHA2.new(256).hexdigest sysusers}'
            '#{Digest::SHA2.new(256).hexdigest conf}')

backup=('etc/#{pkgname}.toml')

build() {
  cargo build --release
}

check() {
  true
}

package() {
  install -Dm600 -o 248 -g 248 "#{pkgname}.toml" "$pkgdir"/etc/#{pkgname}.toml
  install -Dm644 "#{pkgname}.service" "$pkgdir"/usr/lib/systemd/system/#{pkgname}.service
  install -Dm644 "#{pkgname}.sysusers" "${pkgdir}/usr/lib/sysusers.d/${pkgname}.conf"

  install -d -m 700 -o 248 -g 248 "$pkgdir"/var/lib/#{pkgname}/

  find static/ -type f -exec install -Dm600 -o 248 -g 248 {} "$pkgdir"/var/lib/#{pkgname}/{} \\;

  install -Dm755 "target/release/#{pkgname}" "$pkgdir/usr/bin/#{pkgname}"
}
}
File.write("archpkg/PKGBUILD", pkgbuild)

Dir.chdir "archpkg/"
system("makepkg", "-f")
