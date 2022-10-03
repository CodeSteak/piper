rm -Rf archpkg
ruby genarchpkg.rb

rm -v ~/workspace/server/netcup/tasks/tarcloud/*.tar.zst
cp -v archpkg/tarcloud-*-x86_64.pkg.tar.zst ~/workspace/server/netcup/tasks/tarcloud/ 
