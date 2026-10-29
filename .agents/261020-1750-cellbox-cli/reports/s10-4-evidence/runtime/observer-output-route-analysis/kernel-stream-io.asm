
/tmp/cellbox-s10-3d-resume/cand/desktop.kernel.elf:     file format elf64-x86-64


Disassembly of section .text:

ffffffff80051040 <cellos_kernel::task::cell_session::stream_io>:
ffffffff80051040:	55                   	push   %rbp
ffffffff80051041:	41 57                	push   %r15
ffffffff80051043:	41 56                	push   %r14
ffffffff80051045:	41 55                	push   %r13
ffffffff80051047:	41 54                	push   %r12
ffffffff80051049:	53                   	push   %rbx
ffffffff8005104a:	48 81 ec 68 02 00 00 	sub    $0x268,%rsp
ffffffff80051051:	48 8b 84 24 a0 02 00 	mov    0x2a0(%rsp),%rax
ffffffff80051058:	00 
ffffffff80051059:	48 83 f8 08          	cmp    $0x8,%rax
ffffffff8005105d:	0f 82 d6 00 00 00    	jb     ffffffff80051139 <cellos_kernel::task::cell_session::stream_io+0xf9>
ffffffff80051063:	48 83 f8 0f          	cmp    $0xf,%rax
ffffffff80051067:	0f 86 cc 00 00 00    	jbe    ffffffff80051139 <cellos_kernel::task::cell_session::stream_io+0xf9>
ffffffff8005106d:	48 83 f8 14          	cmp    $0x14,%rax
ffffffff80051071:	0f 82 c2 00 00 00    	jb     ffffffff80051139 <cellos_kernel::task::cell_session::stream_io+0xf9>
ffffffff80051077:	48 83 f8 18          	cmp    $0x18,%rax
ffffffff8005107b:	0f 82 b8 00 00 00    	jb     ffffffff80051139 <cellos_kernel::task::cell_session::stream_io+0xf9>
ffffffff80051081:	41 83 79 14 00       	cmpl   $0x0,0x14(%r9)
ffffffff80051086:	0f 85 ad 00 00 00    	jne    ffffffff80051139 <cellos_kernel::task::cell_session::stream_io+0xf9>
ffffffff8005108c:	48 83 f8 1f          	cmp    $0x1f,%rax
ffffffff80051090:	0f 86 a3 00 00 00    	jbe    ffffffff80051139 <cellos_kernel::task::cell_session::stream_io+0xf9>
ffffffff80051096:	45 8b 71 10          	mov    0x10(%r9),%r14d
ffffffff8005109a:	45 8d 56 ff          	lea    -0x1(%r14),%r10d
ffffffff8005109e:	41 81 fa 00 02 00 00 	cmp    $0x200,%r10d
ffffffff800510a5:	0f 83 8e 00 00 00    	jae    ffffffff80051139 <cellos_kernel::task::cell_session::stream_io+0xf9>
ffffffff800510ab:	4d 8b 39             	mov    (%r9),%r15
ffffffff800510ae:	49 8b 59 08          	mov    0x8(%r9),%rbx
ffffffff800510b2:	49 8b 69 18          	mov    0x18(%r9),%rbp
ffffffff800510b6:	66 41 83 f8 06       	cmp    $0x6,%r8w
ffffffff800510bb:	75 12                	jne    ffffffff800510cf <cellos_kernel::task::cell_session::stream_io+0x8f>
ffffffff800510bd:	4d 8d 56 20          	lea    0x20(%r14),%r10
ffffffff800510c1:	49 39 c2             	cmp    %rax,%r10
ffffffff800510c4:	77 73                	ja     ffffffff80051139 <cellos_kernel::task::cell_session::stream_io+0xf9>
ffffffff800510c6:	49 83 c1 20          	add    $0x20,%r9
ffffffff800510ca:	4d 89 f3             	mov    %r14,%r11
ffffffff800510cd:	eb 0f                	jmp    ffffffff800510de <cellos_kernel::task::cell_session::stream_io+0x9e>
ffffffff800510cf:	41 b9 01 00 00 00    	mov    $0x1,%r9d
ffffffff800510d5:	41 ba 20 00 00 00    	mov    $0x20,%r10d
ffffffff800510db:	45 31 db             	xor    %r11d,%r11d
ffffffff800510de:	49 39 c2             	cmp    %rax,%r10
ffffffff800510e1:	75 56                	jne    ffffffff80051139 <cellos_kernel::task::cell_session::stream_io+0xf9>
ffffffff800510e3:	4c 89 74 24 30       	mov    %r14,0x30(%rsp)
ffffffff800510e8:	4c 89 5c 24 38       	mov    %r11,0x38(%rsp)
ffffffff800510ed:	48 89 74 24 20       	mov    %rsi,0x20(%rsp)
ffffffff800510f2:	48 89 54 24 60       	mov    %rdx,0x60(%rsp)
ffffffff800510f7:	48 89 4c 24 58       	mov    %rcx,0x58(%rsp)
ffffffff800510fc:	4c 89 4c 24 40       	mov    %r9,0x40(%rsp)
ffffffff80051101:	48 89 7c 24 10       	mov    %rdi,0x10(%rsp)
ffffffff80051106:	31 c0                	xor    %eax,%eax
ffffffff80051108:	44 89 44 24 1c       	mov    %r8d,0x1c(%rsp)
ffffffff8005110d:	66 41 83 f8 05       	cmp    $0x5,%r8w
ffffffff80051112:	0f 94 c0             	sete   %al
ffffffff80051115:	b9 02 00 00 00       	mov    $0x2,%ecx
ffffffff8005111a:	29 c1                	sub    %eax,%ecx
ffffffff8005111c:	89 4c 24 2c          	mov    %ecx,0x2c(%rsp)
ffffffff80051120:	b1 08                	mov    $0x8,%cl
ffffffff80051122:	28 c1                	sub    %al,%cl
ffffffff80051124:	88 4c 24 0f          	mov    %cl,0xf(%rsp)
ffffffff80051128:	4c 8d 6c 24 68       	lea    0x68(%rsp),%r13
ffffffff8005112d:	48 89 5c 24 50       	mov    %rbx,0x50(%rsp)
ffffffff80051132:	48 89 6c 24 48       	mov    %rbp,0x48(%rsp)
ffffffff80051137:	eb 2f                	jmp    ffffffff80051168 <cellos_kernel::task::cell_session::stream_io+0x128>
ffffffff80051139:	c7 07 01 00 03 00    	movl   $0x30001,(%rdi)
ffffffff8005113f:	48 81 c4 68 02 00 00 	add    $0x268,%rsp
ffffffff80051146:	5b                   	pop    %rbx
ffffffff80051147:	41 5c                	pop    %r12
ffffffff80051149:	41 5d                	pop    %r13
ffffffff8005114b:	41 5e                	pop    %r14
ffffffff8005114d:	41 5f                	pop    %r15
ffffffff8005114f:	5d                   	pop    %rbp
ffffffff80051150:	c3                   	ret
ffffffff80051151:	e8 fa 4b 06 00       	call   ffffffff800b5d50 <cellos_kernel::task::yield_cpu>
ffffffff80051156:	49 89 df             	mov    %rbx,%r15
ffffffff80051159:	48 8b 5c 24 50       	mov    0x50(%rsp),%rbx
ffffffff8005115e:	48 8b 6c 24 48       	mov    0x48(%rsp),%rbp
ffffffff80051163:	4c 8d 6c 24 68       	lea    0x68(%rsp),%r13
ffffffff80051168:	e8 43 4f 0a 00       	call   ffffffff800f60b0 <<cellos_kernel::sync::Spinlock<core::option::Option<cellos_kernel::task::scheduler::Scheduler>>>::lock>
ffffffff8005116d:	41 89 c6             	mov    %eax,%r14d
ffffffff80051170:	48 83 3d 70 e0 9b 01 	cmpq   $0xffffffffffffffff,0x19be070(%rip)        # ffffffff81a0f1e8 <cellos_kernel::task::SCHEDULER>
ffffffff80051177:	ff 
ffffffff80051178:	0f 84 8a 02 00 00    	je     ffffffff80051408 <cellos_kernel::task::cell_session::stream_io+0x3c8>
ffffffff8005117e:	4c 89 ef             	mov    %r13,%rdi
ffffffff80051181:	48 8b 74 24 20       	mov    0x20(%rsp),%rsi
ffffffff80051186:	e8 05 04 00 00       	call   ffffffff80051590 <cellos_kernel::task::cell_session::resolve_actor>
ffffffff8005118b:	80 7c 24 68 00       	cmpb   $0x0,0x68(%rsp)
ffffffff80051190:	0f 85 1a 03 00 00    	jne    ffffffff800514b0 <cellos_kernel::task::cell_session::stream_io+0x470>
ffffffff80051196:	48 8b 54 24 60       	mov    0x60(%rsp),%rdx
ffffffff8005119b:	48 39 54 24 70       	cmp    %rdx,0x70(%rsp)
ffffffff800511a0:	48 8b 4c 24 58       	mov    0x58(%rsp),%rcx
ffffffff800511a5:	0f 85 5d 02 00 00    	jne    ffffffff80051408 <cellos_kernel::task::cell_session::stream_io+0x3c8>
ffffffff800511ab:	48 39 4c 24 78       	cmp    %rcx,0x78(%rsp)
ffffffff800511b0:	0f 85 52 02 00 00    	jne    ffffffff80051408 <cellos_kernel::task::cell_session::stream_io+0x3c8>
ffffffff800511b6:	8b 44 24 2c          	mov    0x2c(%rsp),%eax
ffffffff800511ba:	89 04 24             	mov    %eax,(%rsp)
ffffffff800511bd:	4c 89 ef             	mov    %r13,%rdi
ffffffff800511c0:	48 c7 c6 38 f2 a0 81 	mov    $0xffffffff81a0f238,%rsi
ffffffff800511c7:	4d 89 f8             	mov    %r15,%r8
ffffffff800511ca:	49 89 d9             	mov    %rbx,%r9
ffffffff800511cd:	e8 8e 04 00 00       	call   ffffffff80051660 <<cellos_kernel::task::cell_session::State>::endpoint>
ffffffff800511d2:	83 7c 24 68 ff       	cmpl   $0xffffffff,0x68(%rsp)
ffffffff800511d7:	0f 84 da 02 00 00    	je     ffffffff800514b7 <cellos_kernel::task::cell_session::stream_io+0x477>
ffffffff800511dd:	44 88 74 24 0e       	mov    %r14b,0xe(%rsp)
ffffffff800511e2:	4c 89 fb             	mov    %r15,%rbx
ffffffff800511e5:	4c 8b bc 24 90 00 00 	mov    0x90(%rsp),%r15
ffffffff800511ec:	00 
ffffffff800511ed:	44 0f b6 b4 24 98 00 	movzbl 0x98(%rsp),%r14d
ffffffff800511f4:	00 00 
ffffffff800511f6:	ba 00 02 00 00       	mov    $0x200,%edx
ffffffff800511fb:	4c 89 ef             	mov    %r13,%rdi
ffffffff800511fe:	31 f6                	xor    %esi,%esi
ffffffff80051200:	ff 15 1a e4 9b 01    	call   *0x19be41a(%rip)        # ffffffff81a0f620 <log::LOGGER+0x20>
ffffffff80051206:	48 8b 3d e3 df 9b 01 	mov    0x19bdfe3(%rip),%rdi        # ffffffff81a0f1f0 <cellos_kernel::task::SCHEDULER+0x8>
ffffffff8005120d:	48 8b 35 e4 df 9b 01 	mov    0x19bdfe4(%rip),%rsi        # ffffffff81a0f1f8 <cellos_kernel::task::SCHEDULER+0x10>
ffffffff80051214:	66 83 7c 24 1c 05    	cmpw   $0x5,0x1c(%rsp)
ffffffff8005121a:	75 25                	jne    ffffffff80051241 <cellos_kernel::task::cell_session::stream_io+0x201>
ffffffff8005121c:	45 89 f4             	mov    %r14d,%r12d
ffffffff8005121f:	41 0f b6 ce          	movzbl %r14b,%ecx
ffffffff80051223:	4c 89 fa             	mov    %r15,%rdx
ffffffff80051226:	4d 89 e8             	mov    %r13,%r8
ffffffff80051229:	4c 8b 4c 24 30       	mov    0x30(%rsp),%r9
ffffffff8005122e:	e8 cd 5f 0c 00       	call   ffffffff80117200 <cellos_kernel::task::pipe::session_read>
ffffffff80051233:	48 83 f8 02          	cmp    $0x2,%rax
ffffffff80051237:	4d 89 fd             	mov    %r15,%r13
ffffffff8005123a:	74 2b                	je     ffffffff80051267 <cellos_kernel::task::cell_session::stream_io+0x227>
ffffffff8005123c:	e9 17 02 00 00       	jmp    ffffffff80051458 <cellos_kernel::task::cell_session::stream_io+0x418>
ffffffff80051241:	45 89 f4             	mov    %r14d,%r12d
ffffffff80051244:	41 0f b6 ce          	movzbl %r14b,%ecx
ffffffff80051248:	4c 89 fa             	mov    %r15,%rdx
ffffffff8005124b:	4c 8b 44 24 40       	mov    0x40(%rsp),%r8
ffffffff80051250:	4c 8b 4c 24 38       	mov    0x38(%rsp),%r9
ffffffff80051255:	e8 86 6e 0c 00       	call   ffffffff801180e0 <cellos_kernel::task::pipe::session_write>
ffffffff8005125a:	48 83 f8 01          	cmp    $0x1,%rax
ffffffff8005125e:	4d 89 fd             	mov    %r15,%r13
ffffffff80051261:	0f 85 b1 01 00 00    	jne    ffffffff80051418 <cellos_kernel::task::cell_session::stream_io+0x3d8>
ffffffff80051267:	48 83 fd ff          	cmp    $0xffffffffffffffff,%rbp
ffffffff8005126b:	48 8b 74 24 10       	mov    0x10(%rsp),%rsi
ffffffff80051270:	74 3d                	je     ffffffff800512af <cellos_kernel::task::cell_session::stream_io+0x26f>
ffffffff80051272:	48 85 ed             	test   %rbp,%rbp
ffffffff80051275:	0f 84 ab 02 00 00    	je     ffffffff80051526 <cellos_kernel::task::cell_session::stream_io+0x4e6>
ffffffff8005127b:	48 8b 05 e6 7c a4 01 	mov    0x1a47ce6(%rip),%rax        # ffffffff81a98f68 <hal_x86::x86_64::hpet::HPET_PERIOD_FS>
ffffffff80051282:	48 85 c0             	test   %rax,%rax
ffffffff80051285:	74 31                	je     ffffffff800512b8 <cellos_kernel::task::cell_session::stream_io+0x278>
ffffffff80051287:	48 8b 0d e2 7c a4 01 	mov    0x1a47ce2(%rip),%rcx        # ffffffff81a98f70 <hal_x86::x86_64::hpet::HPET_BASE>
ffffffff8005128e:	48 f7 a1 f0 00 00 00 	mulq   0xf0(%rcx)
ffffffff80051295:	48 89 d6             	mov    %rdx,%rsi
ffffffff80051298:	ba 40 42 0f 00       	mov    $0xf4240,%edx
ffffffff8005129d:	48 89 c7             	mov    %rax,%rdi
ffffffff800512a0:	31 c9                	xor    %ecx,%ecx
ffffffff800512a2:	ff 15 88 e3 9b 01    	call   *0x19be388(%rip)        # ffffffff81a0f630 <log::LOGGER+0x30>
ffffffff800512a8:	48 8b 74 24 10       	mov    0x10(%rsp),%rsi
ffffffff800512ad:	eb 0b                	jmp    ffffffff800512ba <cellos_kernel::task::cell_session::stream_io+0x27a>
ffffffff800512af:	45 31 ff             	xor    %r15d,%r15d
ffffffff800512b2:	8b 7c 24 1c          	mov    0x1c(%rsp),%edi
ffffffff800512b6:	eb 6b                	jmp    ffffffff80051323 <cellos_kernel::task::cell_session::stream_io+0x2e3>
ffffffff800512b8:	31 c0                	xor    %eax,%eax
ffffffff800512ba:	8b 7c 24 1c          	mov    0x1c(%rsp),%edi
ffffffff800512be:	48 29 c5             	sub    %rax,%rbp
ffffffff800512c1:	0f 86 6a 02 00 00    	jbe    ffffffff80051531 <cellos_kernel::task::cell_session::stream_io+0x4f1>
ffffffff800512c7:	48 89 e8             	mov    %rbp,%rax
ffffffff800512ca:	48 ba bd 42 7a e5 d5 	movabs $0xd6bf94d5e57a42bd,%rdx
ffffffff800512d1:	94 bf d6 
ffffffff800512d4:	48 f7 e2             	mul    %rdx
ffffffff800512d7:	48 c1 ea 17          	shr    $0x17,%rdx
ffffffff800512db:	48 b8 a5 46 8d ae 77 	movabs $0xe5032477ae8d46a5,%rax
ffffffff800512e2:	24 03 e5 
ffffffff800512e5:	48 0f af e8          	imul   %rax,%rbp
ffffffff800512e9:	48 c1 cd 07          	ror    $0x7,%rbp
ffffffff800512ed:	31 c0                	xor    %eax,%eax
ffffffff800512ef:	49 b8 ca ab 29 7f ad 	movabs $0x1ad7f29abca,%r8
ffffffff800512f6:	01 00 00 
ffffffff800512f9:	4c 39 c5             	cmp    %r8,%rbp
ffffffff800512fc:	0f 97 c0             	seta   %al
ffffffff800512ff:	48 01 d0             	add    %rdx,%rax
ffffffff80051302:	48 8b 2d 9f 2b a4 01 	mov    0x1a42b9f(%rip),%rbp        # ffffffff81a93ea8 <cellos_kernel::task::TICKS>
ffffffff80051309:	48 01 c5             	add    %rax,%rbp
ffffffff8005130c:	0f 92 c0             	setb   %al
ffffffff8005130f:	48 ff c5             	inc    %rbp
ffffffff80051312:	0f 94 c1             	sete   %cl
ffffffff80051315:	08 c1                	or     %al,%cl
ffffffff80051317:	41 bf 01 00 00 00    	mov    $0x1,%r15d
ffffffff8005131d:	0f 85 c8 00 00 00    	jne    ffffffff800513eb <cellos_kernel::task::cell_session::stream_io+0x3ab>
ffffffff80051323:	41 0f b6 f4          	movzbl %r12b,%esi
ffffffff80051327:	66 83 ff 05          	cmp    $0x5,%di
ffffffff8005132b:	75 0f                	jne    ffffffff8005133c <cellos_kernel::task::cell_session::stream_io+0x2fc>
ffffffff8005132d:	4c 89 ef             	mov    %r13,%rdi
ffffffff80051330:	48 8b 54 24 20       	mov    0x20(%rsp),%rdx
ffffffff80051335:	e8 46 5b 0c 00       	call   ffffffff80116e80 <cellos_kernel::task::pipe::park_reader>
ffffffff8005133a:	eb 0d                	jmp    ffffffff80051349 <cellos_kernel::task::cell_session::stream_io+0x309>
ffffffff8005133c:	4c 89 ef             	mov    %r13,%rdi
ffffffff8005133f:	48 8b 54 24 20       	mov    0x20(%rsp),%rdx
ffffffff80051344:	e8 f7 5c 0c 00       	call   ffffffff80117040 <cellos_kernel::task::pipe::park_writer>
ffffffff80051349:	48 8b 05 a0 de 9b 01 	mov    0x19bdea0(%rip),%rax        # ffffffff81a0f1f0 <cellos_kernel::task::SCHEDULER+0x8>
ffffffff80051350:	48 8b 35 a1 de 9b 01 	mov    0x19bdea1(%rip),%rsi        # ffffffff81a0f1f8 <cellos_kernel::task::SCHEDULER+0x10>
ffffffff80051357:	48 83 fe 01          	cmp    $0x1,%rsi
ffffffff8005135b:	48 8b 4c 24 10       	mov    0x10(%rsp),%rcx
ffffffff80051360:	74 36                	je     ffffffff80051398 <cellos_kernel::task::cell_session::stream_io+0x358>
ffffffff80051362:	48 85 f6             	test   %rsi,%rsi
ffffffff80051365:	48 8b 54 24 20       	mov    0x20(%rsp),%rdx
ffffffff8005136a:	0f 84 70 01 00 00    	je     ffffffff800514e0 <cellos_kernel::task::cell_session::stream_io+0x4a0>
ffffffff80051370:	31 ff                	xor    %edi,%edi
ffffffff80051372:	49 89 f8             	mov    %rdi,%r8
ffffffff80051375:	49 89 f1             	mov    %rsi,%r9
ffffffff80051378:	49 d1 e9             	shr    $1,%r9
ffffffff8005137b:	4c 01 cf             	add    %r9,%rdi
ffffffff8005137e:	49 89 fa             	mov    %rdi,%r10
ffffffff80051381:	49 c1 e2 04          	shl    $0x4,%r10
ffffffff80051385:	4a 39 14 10          	cmp    %rdx,(%rax,%r10,1)
ffffffff80051389:	49 0f 47 f8          	cmova  %r8,%rdi
ffffffff8005138d:	4c 29 ce             	sub    %r9,%rsi
ffffffff80051390:	48 83 fe 01          	cmp    $0x1,%rsi
ffffffff80051394:	77 dc                	ja     ffffffff80051372 <cellos_kernel::task::cell_session::stream_io+0x332>
ffffffff80051396:	eb 07                	jmp    ffffffff8005139f <cellos_kernel::task::cell_session::stream_io+0x35f>
ffffffff80051398:	31 ff                	xor    %edi,%edi
ffffffff8005139a:	48 8b 54 24 20       	mov    0x20(%rsp),%rdx
ffffffff8005139f:	48 c1 e7 04          	shl    $0x4,%rdi
ffffffff800513a3:	48 39 14 38          	cmp    %rdx,(%rax,%rdi,1)
ffffffff800513a7:	0f 85 33 01 00 00    	jne    ffffffff800514e0 <cellos_kernel::task::cell_session::stream_io+0x4a0>
ffffffff800513ad:	48 01 f8             	add    %rdi,%rax
ffffffff800513b0:	41 80 e4 01          	and    $0x1,%r12b
ffffffff800513b4:	48 8b 40 08          	mov    0x8(%rax),%rax
ffffffff800513b8:	0f b6 4c 24 0f       	movzbl 0xf(%rsp),%ecx
ffffffff800513bd:	88 48 60             	mov    %cl,0x60(%rax)
ffffffff800513c0:	4c 89 68 68          	mov    %r13,0x68(%rax)
ffffffff800513c4:	44 88 60 70          	mov    %r12b,0x70(%rax)
ffffffff800513c8:	4c 89 78 78          	mov    %r15,0x78(%rax)
ffffffff800513cc:	48 89 a8 80 00 00 00 	mov    %rbp,0x80(%rax)
ffffffff800513d3:	c6 05 16 e0 9b 01 00 	movb   $0x0,0x19be016(%rip)        # ffffffff81a0f3f0 <cellos_kernel::task::SCHEDULER+0x208>
ffffffff800513da:	f6 44 24 0e 01       	testb  $0x1,0xe(%rsp)
ffffffff800513df:	0f 84 6c fd ff ff    	je     ffffffff80051151 <cellos_kernel::task::cell_session::stream_io+0x111>
ffffffff800513e5:	fb                   	sti
ffffffff800513e6:	e9 66 fd ff ff       	jmp    ffffffff80051151 <cellos_kernel::task::cell_session::stream_io+0x111>
ffffffff800513eb:	c7 06 01 00 03 00    	movl   $0x30001,(%rsi)
ffffffff800513f1:	c6 05 f8 df 9b 01 00 	movb   $0x0,0x19bdff8(%rip)        # ffffffff81a0f3f0 <cellos_kernel::task::SCHEDULER+0x208>
ffffffff800513f8:	f6 44 24 0e 01       	testb  $0x1,0xe(%rsp)
ffffffff800513fd:	0f 84 3c fd ff ff    	je     ffffffff8005113f <cellos_kernel::task::cell_session::stream_io+0xff>
ffffffff80051403:	e9 6a 01 00 00       	jmp    ffffffff80051572 <cellos_kernel::task::cell_session::stream_io+0x532>
ffffffff80051408:	48 8b 44 24 10       	mov    0x10(%rsp),%rax
ffffffff8005140d:	c7 00 01 00 11 00    	movl   $0x110001,(%rax)
ffffffff80051413:	e9 b2 00 00 00       	jmp    ffffffff800514ca <cellos_kernel::task::cell_session::stream_io+0x48a>
ffffffff80051418:	48 83 f8 ff          	cmp    $0xffffffffffffffff,%rax
ffffffff8005141c:	48 8b 4c 24 10       	mov    0x10(%rsp),%rcx
ffffffff80051421:	0f b6 74 24 0e       	movzbl 0xe(%rsp),%esi
ffffffff80051426:	0f 84 bf 00 00 00    	je     ffffffff800514eb <cellos_kernel::task::cell_session::stream_io+0x4ab>
ffffffff8005142c:	48 85 c0             	test   %rax,%rax
ffffffff8005142f:	0f 85 cb 00 00 00    	jne    ffffffff80051500 <cellos_kernel::task::cell_session::stream_io+0x4c0>
ffffffff80051435:	48 8b 84 24 a8 02 00 	mov    0x2a8(%rsp),%rax
ffffffff8005143c:	00 
ffffffff8005143d:	89 10                	mov    %edx,(%rax)
ffffffff8005143f:	c7 40 04 00 00 00 00 	movl   $0x0,0x4(%rax)
ffffffff80051446:	48 c7 41 08 08 00 00 	movq   $0x8,0x8(%rcx)
ffffffff8005144d:	00 
ffffffff8005144e:	66 c7 01 00 00       	movw   $0x0,(%rcx)
ffffffff80051453:	e9 ae 00 00 00       	jmp    ffffffff80051506 <cellos_kernel::task::cell_session::stream_io+0x4c6>
ffffffff80051458:	48 83 f8 ff          	cmp    $0xffffffffffffffff,%rax
ffffffff8005145c:	0f b6 6c 24 0e       	movzbl 0xe(%rsp),%ebp
ffffffff80051461:	0f 84 8c 00 00 00    	je     ffffffff800514f3 <cellos_kernel::task::cell_session::stream_io+0x4b3>
ffffffff80051467:	48 85 c0             	test   %rax,%rax
ffffffff8005146a:	0f 85 a9 00 00 00    	jne    ffffffff80051519 <cellos_kernel::task::cell_session::stream_io+0x4d9>
ffffffff80051470:	48 8b 84 24 a8 02 00 	mov    0x2a8(%rsp),%rax
ffffffff80051477:	00 
ffffffff80051478:	89 10                	mov    %edx,(%rax)
ffffffff8005147a:	c7 40 04 00 00 00 00 	movl   $0x0,0x4(%rax)
ffffffff80051481:	48 81 fa 01 02 00 00 	cmp    $0x201,%rdx
ffffffff80051488:	0f 83 ea 00 00 00    	jae    ffffffff80051578 <cellos_kernel::task::cell_session::stream_io+0x538>
ffffffff8005148e:	4c 8d 72 08          	lea    0x8(%rdx),%r14
ffffffff80051492:	4c 3b b4 24 b0 02 00 	cmp    0x2b0(%rsp),%r14
ffffffff80051499:	00 
ffffffff8005149a:	0f 86 9c 00 00 00    	jbe    ffffffff8005153c <cellos_kernel::task::cell_session::stream_io+0x4fc>
ffffffff800514a0:	48 8b 44 24 10       	mov    0x10(%rsp),%rax
ffffffff800514a5:	c7 00 01 00 18 00    	movl   $0x180001,(%rax)
ffffffff800514ab:	e9 b1 00 00 00       	jmp    ffffffff80051561 <cellos_kernel::task::cell_session::stream_io+0x521>
ffffffff800514b0:	0f b7 44 24 6a       	movzwl 0x6a(%rsp),%eax
ffffffff800514b5:	eb 05                	jmp    ffffffff800514bc <cellos_kernel::task::cell_session::stream_io+0x47c>
ffffffff800514b7:	0f b7 44 24 70       	movzwl 0x70(%rsp),%eax
ffffffff800514bc:	48 8b 4c 24 10       	mov    0x10(%rsp),%rcx
ffffffff800514c1:	66 89 41 02          	mov    %ax,0x2(%rcx)
ffffffff800514c5:	66 c7 01 01 00       	movw   $0x1,(%rcx)
ffffffff800514ca:	c6 05 1f df 9b 01 00 	movb   $0x0,0x19bdf1f(%rip)        # ffffffff81a0f3f0 <cellos_kernel::task::SCHEDULER+0x208>
ffffffff800514d1:	41 f6 c6 01          	test   $0x1,%r14b
ffffffff800514d5:	0f 84 64 fc ff ff    	je     ffffffff8005113f <cellos_kernel::task::cell_session::stream_io+0xff>
ffffffff800514db:	e9 92 00 00 00       	jmp    ffffffff80051572 <cellos_kernel::task::cell_session::stream_io+0x532>
ffffffff800514e0:	c7 01 01 00 11 00    	movl   $0x110001,(%rcx)
ffffffff800514e6:	e9 06 ff ff ff       	jmp    ffffffff800513f1 <cellos_kernel::task::cell_session::stream_io+0x3b1>
ffffffff800514eb:	c7 01 01 00 07 00    	movl   $0x70001,(%rcx)
ffffffff800514f1:	eb 13                	jmp    ffffffff80051506 <cellos_kernel::task::cell_session::stream_io+0x4c6>
ffffffff800514f3:	48 8b 44 24 10       	mov    0x10(%rsp),%rax
ffffffff800514f8:	c7 00 01 00 07 00    	movl   $0x70001,(%rax)
ffffffff800514fe:	eb 61                	jmp    ffffffff80051561 <cellos_kernel::task::cell_session::stream_io+0x521>
ffffffff80051500:	c7 01 01 00 12 00    	movl   $0x120001,(%rcx)
ffffffff80051506:	c6 05 e3 de 9b 01 00 	movb   $0x0,0x19bdee3(%rip)        # ffffffff81a0f3f0 <cellos_kernel::task::SCHEDULER+0x208>
ffffffff8005150d:	40 f6 c6 01          	test   $0x1,%sil
ffffffff80051511:	0f 84 28 fc ff ff    	je     ffffffff8005113f <cellos_kernel::task::cell_session::stream_io+0xff>
ffffffff80051517:	eb 59                	jmp    ffffffff80051572 <cellos_kernel::task::cell_session::stream_io+0x532>
ffffffff80051519:	48 8b 44 24 10       	mov    0x10(%rsp),%rax
ffffffff8005151e:	c7 00 01 00 01 00    	movl   $0x10001,(%rax)
ffffffff80051524:	eb 3b                	jmp    ffffffff80051561 <cellos_kernel::task::cell_session::stream_io+0x521>
ffffffff80051526:	c7 06 01 00 0e 00    	movl   $0xe0001,(%rsi)
ffffffff8005152c:	e9 c0 fe ff ff       	jmp    ffffffff800513f1 <cellos_kernel::task::cell_session::stream_io+0x3b1>
ffffffff80051531:	c7 06 01 00 0f 00    	movl   $0xf0001,(%rsi)
ffffffff80051537:	e9 b5 fe ff ff       	jmp    ffffffff800513f1 <cellos_kernel::task::cell_session::stream_io+0x3b1>
ffffffff8005153c:	48 8b bc 24 a8 02 00 	mov    0x2a8(%rsp),%rdi
ffffffff80051543:	00 
ffffffff80051544:	48 83 c7 08          	add    $0x8,%rdi
ffffffff80051548:	48 8d 74 24 68       	lea    0x68(%rsp),%rsi
ffffffff8005154d:	ff 15 bd e0 9b 01    	call   *0x19be0bd(%rip)        # ffffffff81a0f610 <log::LOGGER+0x10>
ffffffff80051553:	48 8b 44 24 10       	mov    0x10(%rsp),%rax
ffffffff80051558:	4c 89 70 08          	mov    %r14,0x8(%rax)
ffffffff8005155c:	66 c7 00 00 00       	movw   $0x0,(%rax)
ffffffff80051561:	c6 05 88 de 9b 01 00 	movb   $0x0,0x19bde88(%rip)        # ffffffff81a0f3f0 <cellos_kernel::task::SCHEDULER+0x208>
ffffffff80051568:	40 f6 c5 01          	test   $0x1,%bpl
ffffffff8005156c:	0f 84 cd fb ff ff    	je     ffffffff8005113f <cellos_kernel::task::cell_session::stream_io+0xff>
ffffffff80051572:	fb                   	sti
ffffffff80051573:	e9 c7 fb ff ff       	jmp    ffffffff8005113f <cellos_kernel::task::cell_session::stream_io+0xff>
ffffffff80051578:	48 89 d6             	mov    %rdx,%rsi
ffffffff8005157b:	ba 00 02 00 00       	mov    $0x200,%edx
ffffffff80051580:	31 ff                	xor    %edi,%edi
ffffffff80051582:	48 c7 c1 d8 2f 19 80 	mov    $0xffffffff80192fd8,%rcx
ffffffff80051589:	e8 42 3b 0d 00       	call   ffffffff801250d0 <core::slice::index::slice_index_fail>
