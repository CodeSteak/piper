#!/bin/sh
set -ev

rm -Rf archpkg
ruby genarchpkg.rb

rm -v ~/workspace/server/netcup/tasks/tarcloud/*.tar.zst || true
cp -v archpkg/tarcloud-*-x86_64.pkg.tar.zst ~/workspace/server/netcup/tasks/tarcloud/ 

cd ~/workspace/server/netcup/
/home/robin/.local/bin/pyinfra -v inventory.py all.py
ssh root@pluto.willmann.dev 'systemctl restart tarcloud'