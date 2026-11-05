// SPDX-License-Identifier: MIT
// Cellos (vicell) sysdeps for mlibc: the set of sysdep tags this OS implements.
//
// mlibc dispatches every OS hook through `mlibc::sysdep<Tag>()` and
// `Sysdeps<Tag>` (options/internal/include/mlibc/all-sysdeps.hpp). A tag listed
// here must have a `Sysdeps<Tag>::operator()` definition under generic/; an
// unlisted tag resolves to `NoImpl`, so mlibc reports it as unimplemented
// (ENOSYS) instead of calling into an undefined symbol.
//
// The `posix` option set is on; `linux`, `glibc` and `bsd` are off
// (meson.build). The CRT is ostd's `_start`, not mlibc's crt1.o.
#pragma once

#include <mlibc/sysdep-signatures.hpp>

namespace mlibc {

struct VicellSysdepTags :
	LibcPanic,
	LibcLog,
	Exit,
	TcbSet,
	FutexWait,
	FutexWake,
	Open,
	Read,
	Write,
	Close,
	Seek,
	ClockGet,
	AnonAllocate,
	AnonFree,
	VmMap,
	VmUnmap,
	Isatty,
	Stat,
	// generic/generic.cpp: fd providers (vicell/fd_provider.h); reached
	// through posix_devctl, since `ioctl` itself is a glibc-option function
	Ioctl,
	// generic/thread.cpp
	PrepareStack,
	Clone,
	ThreadExit,
	Yield,
	GetTid,
	// generic/posix.cpp
	Sleep,
	GetPid,
	Sysconf,
	GetEntropy
{};

template<typename Tag>
using Sysdeps = SysdepOf<VicellSysdepTags, Tag>;

struct SysdepTraits {
	static constexpr bool usesRtNetlink = false;
};

} // namespace mlibc
