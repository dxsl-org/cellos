-----BEGIN PGP SIGNED MESSAGE-----
Hash: SHA512

Format: 3.0 (quilt)
Source: qemu
Binary: qemu-system, qemu-block-extra, qemu-block-supplemental, qemu-system-data, qemu-system-common, qemu-system-gui, qemu-system-modules-spice, qemu-system-modules-opengl, qemu-system-misc, qemu-system-arm, qemu-system-mips, qemu-system-ppc, qemu-system-sparc, qemu-system-s390x, qemu-system-x86-xen, qemu-system-x86, qemu-system-xen, qemu-user, qemu-user-static, qemu-user-binfmt, qemu-utils, qemu-guest-agent
Architecture: any all
Version: 1:8.2.2+ds-0ubuntu1.17
Maintainer: Ubuntu Developers <ubuntu-devel-discuss@lists.ubuntu.com>
Uploaders: Michael Tokarev <mjt@tls.msk.ru>
Homepage: http://www.qemu.org/
Standards-Version: 4.6.1
Vcs-Browser: https://git.launchpad.net/ubuntu/+source/qemu
Vcs-Git: https://git.launchpad.net/ubuntu/+source/qemu
Testsuite: autopkgtest
Build-Depends: debhelper-compat (= 13), python3:any, meson (>> 0.63.0~), ninja-build, flex, bison
Build-Depends-Arch: python3-venv, texinfo, python3-sphinx:native, python3-sphinx-rtd-theme, libglib2.0-dev, zlib1g-dev, libaio-dev [linux-any], libjack-dev [amd64 arm arm64 armel armhf i386 mips mips64 mips64el mipsel powerpc powerpcspe ppc64 ppc64el riscv64 s390x sparc sparc64 x32], libpulse-dev, libasound2-dev [amd64 arm arm64 armel armhf i386 mips mips64 mips64el mipsel powerpc powerpcspe ppc64 ppc64el riscv64 s390x sparc sparc64 x32], libpipewire-0.3-dev [amd64 arm arm64 armel armhf i386 mips mips64 mips64el mipsel powerpc powerpcspe ppc64 ppc64el riscv64 s390x sparc sparc64 x32], libbpf-dev [linux-any], libbrlapi-dev [amd64 arm arm64 armel armhf i386 mips mips64 mips64el mipsel powerpc powerpcspe ppc64 ppc64el riscv64 s390x sparc sparc64 x32], libcap-ng-dev [linux-any], libcurl4-gnutls-dev, libfdt-dev, libfuse3-dev, gnutls-dev, libgtk-3-dev [amd64 arm arm64 armel armhf i386 mips mips64 mips64el mipsel powerpc powerpcspe ppc64 ppc64el riscv64 s390x sparc sparc64 x32], libvte-2.91-dev [amd64 arm arm64 armel armhf i386 mips mips64 mips64el mipsel powerpc powerpcspe ppc64 ppc64el riscv64 s390x sparc sparc64 x32], libiscsi-dev, libncurses-dev [amd64 arm arm64 armel armhf i386 mips mips64 mips64el mipsel powerpc powerpcspe ppc64 ppc64el riscv64 s390x sparc sparc64 x32], libvirglrenderer-dev [amd64 arm arm64 armel armhf i386 mips mips64 mips64el mipsel powerpc powerpcspe ppc64 ppc64el riscv64 s390x sparc sparc64 x32], libepoxy-dev [amd64 arm arm64 armel armhf i386 mips mips64 mips64el mipsel powerpc powerpcspe ppc64 ppc64el riscv64 s390x sparc sparc64 x32], libdrm-dev [amd64 arm arm64 armel armhf i386 mips mips64 mips64el mipsel powerpc powerpcspe ppc64 ppc64el riscv64 s390x sparc sparc64 x32], libgbm-dev [amd64 arm arm64 armel armhf i386 mips mips64 mips64el mipsel powerpc powerpcspe ppc64 ppc64el riscv64 s390x sparc sparc64 x32], libnfs-dev, libnuma-dev [i386 amd64 ia64 mips mipsel mips64 mips64el powerpc powerpcspe x32 ppc64 ppc64el arm64 sparc s390x riscv64], libcacard-dev, libpixman-1-dev, librbd-dev [amd64 arm64 mips64el ppc64el ppc64 riscv64 s390x sparc64], libglusterfs-dev [amd64 arm64 ppc64el ppc64 riscv64 mips64el s390x ia64 sparc64], libsasl2-dev, libsdl2-dev [amd64 arm arm64 armel armhf i386 mips mips64 mips64el mipsel powerpc powerpcspe ppc64 ppc64el riscv64 s390x sparc sparc64 x32], libseccomp-dev [amd64 arm64 armel armhf i386 mips64el mipsel ppc64 ppc64el powerpc riscv64 s390x x32], libslirp-dev, libspice-server-dev [amd64 arm64 armel armhf i386 mips64el mipsel ppc64el x32], librdmacm-dev [linux-any], libibverbs-dev [linux-any], libibumad-dev [linux-any], liburing-dev [linux-any], libusb-1.0-0-dev [linux-any], libusbredirparser-dev [linux-any], libssh-dev, libzstd-dev, libxen-dev [linux-amd64], nettle-dev, libudev-dev [linux-any], libjpeg-dev, libpng-dev, libpmem-dev [amd64 arm64]
Build-Depends-Indep: device-tree-compiler, gcc-s390x-linux-gnu, gcc-alpha-linux-gnu, gcc-powerpc-linux-gnu, bc, gcc-powerpc64-linux-gnu, libssl-dev, gcc-sparc64-linux-gnu, fcode-utils, xsltproc, gcc-hppa-linux-gnu, gcc-riscv64-linux-gnu, gcc-arm-none-eabi
Build-Conflicts: oss4-dev
Package-List:
 qemu-block-extra deb otherosfs optional arch=amd64,arm,arm64,armel,armhf,hppa,i386,ia64,kfreebsd-amd64,kfreebsd-i386,m68k,mips,mips64,mips64el,mipsel,powerpc,powerpcspe,ppc64,ppc64el,riscv64,s390x,sh4,sparc,sparc64,x32
 qemu-block-supplemental deb otherosfs optional arch=amd64,arm64,ppc64el,riscv64,s390x
 qemu-guest-agent deb otherosfs optional arch=any
 qemu-system deb otherosfs optional arch=amd64,arm,arm64,armel,armhf,i386,mips,mips64,mips64el,mipsel,powerpc,powerpcspe,ppc64,ppc64el,riscv64,s390x,sparc,sparc64,x32 profile=!pkg.qemu.omit-system profile:v1=!pkg.qemu.omit-system
 qemu-system-arm deb otherosfs optional arch=amd64,arm,arm64,armel,armhf,i386,mips,mips64,mips64el,mipsel,powerpc,powerpcspe,ppc64,ppc64el,riscv64,s390x,sparc,sparc64,x32 profile=!pkg.qemu.omit-system profile:v1=!pkg.qemu.omit-system
 qemu-system-common deb otherosfs optional arch=amd64,arm,arm64,armel,armhf,i386,mips,mips64,mips64el,mipsel,powerpc,powerpcspe,ppc64,ppc64el,riscv64,s390x,sparc,sparc64,x32 profile=!pkg.qemu.omit-system profile:v1=!pkg.qemu.omit-system
 qemu-system-data deb otherosfs optional arch=all
 qemu-system-gui deb otherosfs optional arch=amd64,arm,arm64,armel,armhf,i386,mips,mips64,mips64el,mipsel,powerpc,powerpcspe,ppc64,ppc64el,riscv64,s390x,sparc,sparc64,x32
 qemu-system-mips deb otherosfs optional arch=amd64,arm,arm64,armel,armhf,i386,mips,mips64,mips64el,mipsel,powerpc,powerpcspe,ppc64,ppc64el,riscv64,s390x,sparc,sparc64,x32 profile=!pkg.qemu.omit-system profile:v1=!pkg.qemu.omit-system
 qemu-system-misc deb otherosfs optional arch=amd64,arm,arm64,armel,armhf,i386,mips,mips64,mips64el,mipsel,powerpc,powerpcspe,ppc64,ppc64el,riscv64,s390x,sparc,sparc64,x32 profile=!pkg.qemu.omit-system profile:v1=!pkg.qemu.omit-system
 qemu-system-modules-opengl deb otherosfs optional arch=amd64,arm,arm64,armel,armhf,i386,mips,mips64,mips64el,mipsel,powerpc,powerpcspe,ppc64,ppc64el,riscv64,s390x,sparc,sparc64,x32 profile=!pkg.qemu.omit-system profile:v1=!pkg.qemu.omit-system
 qemu-system-modules-spice deb otherosfs optional arch=amd64,arm64,armel,armhf,i386,mips64el,mipsel,ppc64el,x32 profile=!pkg.qemu.omit-system profile:v1=!pkg.qemu.omit-system
 qemu-system-ppc deb otherosfs optional arch=amd64,arm,arm64,armel,armhf,i386,mips,mips64,mips64el,mipsel,powerpc,powerpcspe,ppc64,ppc64el,riscv64,s390x,sparc,sparc64,x32 profile=!pkg.qemu.omit-system profile:v1=!pkg.qemu.omit-system
 qemu-system-s390x deb otherosfs optional arch=amd64,arm,arm64,armel,armhf,i386,mips,mips64,mips64el,mipsel,powerpc,powerpcspe,ppc64,ppc64el,riscv64,s390x,sparc,sparc64,x32 profile=!pkg.qemu.omit-system profile:v1=!pkg.qemu.omit-system
 qemu-system-sparc deb otherosfs optional arch=amd64,arm,arm64,armel,armhf,i386,mips,mips64,mips64el,mipsel,powerpc,powerpcspe,ppc64,ppc64el,riscv64,s390x,sparc,sparc64,x32 profile=!pkg.qemu.omit-system profile:v1=!pkg.qemu.omit-system
 qemu-system-x86 deb otherosfs optional arch=amd64,arm,arm64,armel,armhf,i386,mips,mips64,mips64el,mipsel,powerpc,powerpcspe,ppc64,ppc64el,riscv64,s390x,sparc,sparc64,x32 profile=!pkg.qemu.omit-system profile:v1=!pkg.qemu.omit-system
 qemu-system-x86-xen deb oldlibs optional arch=amd64
 qemu-system-xen deb otherosfs optional arch=amd64 profile=!pkg.qemu.omit-system-xen profile:v1=!pkg.qemu.omit-system-xen
 qemu-user deb otherosfs optional arch=amd64,arm,arm64,armel,armhf,i386,mips,mips64,mips64el,mipsel,ppc64,ppc64el,riscv64,s390x,sparc,sparc64,x32 profile=!pkg.qemu.omit-user profile:v1=!pkg.qemu.omit-user
 qemu-user-binfmt deb otherosfs optional arch=amd64,arm,arm64,armel,armhf,i386,mips,mips64,mips64el,mipsel,ppc64,ppc64el,riscv64,s390x,sparc,sparc64,x32 profile=!pkg.qemu.omit-user profile:v1=!pkg.qemu.omit-user
 qemu-user-static deb otherosfs optional arch=amd64,arm,arm64,armel,armhf,i386,mips,mips64,mips64el,mipsel,ppc64,ppc64el,riscv64,s390x,sparc,sparc64,x32 profile=!pkg.qemu.omit-user-static profile:v1=!pkg.qemu.omit-user-static
 qemu-utils deb otherosfs optional arch=amd64,arm,arm64,armel,armhf,hppa,i386,ia64,kfreebsd-amd64,kfreebsd-i386,m68k,mips,mips64,mips64el,mipsel,powerpc,powerpcspe,ppc64,ppc64el,riscv64,s390x,sh4,sparc,sparc64,x32
Checksums-Sha1:
 fd7b59b6524bfb0c7187664a74d33436aa396bcd 39678456 qemu_8.2.2+ds.orig.tar.xz
 33fa06a98ebb83ce5025bc5d6e03894cedbc2010 259688 qemu_8.2.2+ds-0ubuntu1.17.debian.tar.xz
Checksums-Sha256:
 5e692dd20950a819ff6550cd3974e00319a93f47c0a15f808c58df1a9e4d2478 39678456 qemu_8.2.2+ds.orig.tar.xz
 bd2e6336e97dce02ac0e769a6833e21865229f5e6a09192dc59d59731854d967 259688 qemu_8.2.2+ds-0ubuntu1.17.debian.tar.xz
Files:
 7fde2f90d192da40b6f62190ac131f1f 39678456 qemu_8.2.2+ds.orig.tar.xz
 3b162fa320e91418bb259e4d121196e7 259688 qemu_8.2.2+ds-0ubuntu1.17.debian.tar.xz
Debian-Vcs-Browser: https://salsa.debian.org/qemu-team/qemu
Debian-Vcs-Git: https://salsa.debian.org/qemu-team/qemu.git
Original-Maintainer: Debian QEMU Team <pkg-qemu-devel@lists.alioth.debian.org>

-----BEGIN PGP SIGNATURE-----

iQIzBAEBCgAdFiEEY+78PeFNUUbOfyS/NLitfZUp55MFAmofCqMACgkQNLitfZUp
55PaMw/8D/QnNQXklpad3OCk/arKdoEPwRyEawzr8uzPXOBj8X46UyuURIlDroWc
Hxf3HNWIkGmRvamG2DLpfWqS0wZbxEw2q4Fp7lRFFA5EuH0OopLKf4yxfR15zxFY
Z2NAPoM65njLRvpG60n8MOCaIsadMfH/zYofbD9WeWZx06hRGbG3tol8uaz1CPYB
psu47fEHwG4/wagOiAfDL5DySGhKCfuambzojoZhh67LfzhwLv7LqEHtDkvy6wJz
NYjzwxYcBTcY1+ajbOqiA7KTapK12FSkZt3FYuTs10nBMoAgqP0mn8RN6Mfve8BP
Okn3u2Dyy34IupijsMNQRNlfM0C8yMKo+x4NjT+TGzLZ4GfEH6vXUpcmA7dMnmhI
mQ0gPPO6SIGYs9i1JnxhAmnNhQdq2avHGhKJgIGgAnxh+CNu5Q9hDEKBlp1VU7Av
MN5/yJOUmWSaV+Xgl+9A7eroQiOmGbTDtExjrCQ3ne+ne0WM1J/zXeDxC7iucn0A
maVCMReT2+Ws32Q+xpAMZ1KokvipHTVsblZlLZg/SOZbhzyFHFew8B6t64wqLdSR
PS3OXFYEsnBEc59rdmfxuGeGqEO0pAh8+3eEaX5f/wmWUGkg+oRlfvpI5iX/1RNq
WyRsX3NOG2fCGGntA/4mE3vw9gtsd4ZJs6Ua9/ykcogb2AgPBXQ=
=G2Fh
-----END PGP SIGNATURE-----
