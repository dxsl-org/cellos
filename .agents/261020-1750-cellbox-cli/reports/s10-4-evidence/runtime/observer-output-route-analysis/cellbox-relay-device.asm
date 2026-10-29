
/tmp/cellbox-s10-3d-resume/cand/desktop/files/cellbox:     file format elf64-x86-64


Disassembly of section .text:

0000000000012120 <cellbox::executor::relay_device>:
   12120:	55                   	push   %rbp
   12121:	41 57                	push   %r15
   12123:	41 56                	push   %r14
   12125:	41 55                	push   %r13
   12127:	41 54                	push   %r12
   12129:	53                   	push   %rbx
   1212a:	48 81 ec 98 00 00 00 	sub    $0x98,%rsp
   12131:	48 89 d3             	mov    %rdx,%rbx
   12134:	49 89 f6             	mov    %rsi,%r14
   12137:	b1 01                	mov    $0x1,%cl
   12139:	0f 1f 80 00 00 00 00 	nopl   0x0(%rax)
   12140:	31 c0                	xor    %eax,%eax
   12142:	f0 0f b0 0d be 5e 29 	lock cmpxchg %cl,0x295ebe(%rip)        # 2a8008 <cellbox::executor::FRONTEND_OUTPUT>
   12149:	00 
   1214a:	75 06                	jne    12152 <cellbox::executor::relay_device+0x32>
   1214c:	eb 11                	jmp    1215f <cellbox::executor::relay_device+0x3f>
   1214e:	66 90                	xchg   %ax,%ax
   12150:	f3 90                	pause
   12152:	0f b6 05 af 5e 29 00 	movzbl 0x295eaf(%rip),%eax        # 2a8008 <cellbox::executor::FRONTEND_OUTPUT>
   12159:	84 c0                	test   %al,%al
   1215b:	75 f3                	jne    12150 <cellbox::executor::relay_device+0x30>
   1215d:	eb e1                	jmp    12140 <cellbox::executor::relay_device+0x20>
   1215f:	48 8b 05 b2 5e 29 00 	mov    0x295eb2(%rip),%rax        # 2a8018 <cellbox::executor::FRONTEND_OUTPUT+0x10>
   12166:	48 89 44 24 48       	mov    %rax,0x48(%rsp)
   1216b:	48 8b 05 ae 5e 29 00 	mov    0x295eae(%rip),%rax        # 2a8020 <cellbox::executor::FRONTEND_OUTPUT+0x18>
   12172:	48 89 44 24 50       	mov    %rax,0x50(%rsp)
   12177:	48 8b 05 aa 5e 29 00 	mov    0x295eaa(%rip),%rax        # 2a8028 <cellbox::executor::FRONTEND_OUTPUT+0x20>
   1217e:	48 89 44 24 58       	mov    %rax,0x58(%rsp)
   12183:	48 8b 05 a6 5e 29 00 	mov    0x295ea6(%rip),%rax        # 2a8030 <cellbox::executor::FRONTEND_OUTPUT+0x28>
   1218a:	48 89 44 24 60       	mov    %rax,0x60(%rsp)
   1218f:	48 8b 05 a2 5e 29 00 	mov    0x295ea2(%rip),%rax        # 2a8038 <cellbox::executor::FRONTEND_OUTPUT+0x30>
   12196:	48 89 44 24 28       	mov    %rax,0x28(%rsp)
   1219b:	48 8b 05 9e 5e 29 00 	mov    0x295e9e(%rip),%rax        # 2a8040 <cellbox::executor::FRONTEND_OUTPUT+0x38>
   121a2:	48 89 44 24 30       	mov    %rax,0x30(%rsp)
   121a7:	48 8b 05 9a 5e 29 00 	mov    0x295e9a(%rip),%rax        # 2a8048 <cellbox::executor::FRONTEND_OUTPUT+0x40>
   121ae:	48 89 44 24 38       	mov    %rax,0x38(%rsp)
   121b3:	48 8b 05 96 5e 29 00 	mov    0x295e96(%rip),%rax        # 2a8050 <cellbox::executor::FRONTEND_OUTPUT+0x48>
   121ba:	48 89 44 24 40       	mov    %rax,0x40(%rsp)
   121bf:	f6 05 4a 5e 29 00 01 	testb  $0x1,0x295e4a(%rip)        # 2a8010 <cellbox::executor::FRONTEND_OUTPUT+0x8>
   121c6:	c6 05 3b 5e 29 00 00 	movb   $0x0,0x295e3b(%rip)        # 2a8008 <cellbox::executor::FRONTEND_OUTPUT>
   121cd:	0f 84 7c 00 00 00    	je     1224f <cellbox::executor::relay_device+0x12f>
   121d3:	48 8b 44 24 60       	mov    0x60(%rsp),%rax
   121d8:	48 89 84 24 80 00 00 	mov    %rax,0x80(%rsp)
   121df:	00 
   121e0:	48 8b 44 24 58       	mov    0x58(%rsp),%rax
   121e5:	48 89 44 24 78       	mov    %rax,0x78(%rsp)
   121ea:	48 8b 44 24 48       	mov    0x48(%rsp),%rax
   121ef:	48 8b 4c 24 50       	mov    0x50(%rsp),%rcx
   121f4:	48 89 4c 24 70       	mov    %rcx,0x70(%rsp)
   121f9:	48 89 44 24 68       	mov    %rax,0x68(%rsp)
   121fe:	48 8b 44 24 28       	mov    0x28(%rsp),%rax
   12203:	48 8b 4c 24 30       	mov    0x30(%rsp),%rcx
   12208:	48 89 44 24 08       	mov    %rax,0x8(%rsp)
   1220d:	48 89 4c 24 10       	mov    %rcx,0x10(%rsp)
   12212:	48 8b 44 24 38       	mov    0x38(%rsp),%rax
   12217:	48 89 44 24 18       	mov    %rax,0x18(%rsp)
   1221c:	48 8b 44 24 40       	mov    0x40(%rsp),%rax
   12221:	48 89 44 24 20       	mov    %rax,0x20(%rsp)
   12226:	b1 01                	mov    $0x1,%cl
   12228:	0f 1f 84 00 00 00 00 	nopl   0x0(%rax,%rax,1)
   1222f:	00 
   12230:	31 c0                	xor    %eax,%eax
   12232:	f0 0f b0 0d ce 5d 29 	lock cmpxchg %cl,0x295dce(%rip)        # 2a8008 <cellbox::executor::FRONTEND_OUTPUT>
   12239:	00 
   1223a:	75 06                	jne    12242 <cellbox::executor::relay_device+0x122>
   1223c:	eb 6d                	jmp    122ab <cellbox::executor::relay_device+0x18b>
   1223e:	66 90                	xchg   %ax,%ax
   12240:	f3 90                	pause
   12242:	0f b6 05 bf 5d 29 00 	movzbl 0x295dbf(%rip),%eax        # 2a8008 <cellbox::executor::FRONTEND_OUTPUT>
   12249:	84 c0                	test   %al,%al
   1224b:	75 f3                	jne    12240 <cellbox::executor::relay_device+0x120>
   1224d:	eb e1                	jmp    12230 <cellbox::executor::relay_device+0x110>
   1224f:	48 85 db             	test   %rbx,%rbx
   12252:	74 4e                	je     122a2 <cellbox::executor::relay_device+0x182>
   12254:	40 0f b6 ff          	movzbl %dil,%edi
   12258:	48 ff c7             	inc    %rdi
   1225b:	45 31 c0             	xor    %r8d,%r8d
   1225e:	66 bd 16 00          	mov    $0x16,%bp
   12262:	66 66 66 66 66 2e 0f 	data16 data16 data16 data16 cs nopw 0x0(%rax,%rax,1)
   12269:	1f 84 00 00 00 00 00 
   12270:	48 89 da             	mov    %rbx,%rdx
   12273:	4c 29 c2             	sub    %r8,%rdx
   12276:	4b 8d 34 06          	lea    (%r14,%r8,1),%rsi
   1227a:	b8 6d 00 00 00       	mov    $0x6d,%eax
   1227f:	45 31 d2             	xor    %r10d,%r10d
   12282:	0f 05                	syscall
   12284:	48 85 c0             	test   %rax,%rax
   12287:	0f 88 34 01 00 00    	js     123c1 <cellbox::executor::relay_device+0x2a1>
   1228d:	48 8d 48 ff          	lea    -0x1(%rax),%rcx
   12291:	48 39 d1             	cmp    %rdx,%rcx
   12294:	0f 83 27 01 00 00    	jae    123c1 <cellbox::executor::relay_device+0x2a1>
   1229a:	49 01 c0             	add    %rax,%r8
   1229d:	49 39 d8             	cmp    %rbx,%r8
   122a0:	72 ce                	jb     12270 <cellbox::executor::relay_device+0x150>
   122a2:	66 bd ff ff          	mov    $0xffff,%bp
   122a6:	e9 16 01 00 00       	jmp    123c1 <cellbox::executor::relay_device+0x2a1>
   122ab:	48 83 3d 5d 5d 29 00 	cmpq   $0x0,0x295d5d(%rip)        # 2a8010 <cellbox::executor::FRONTEND_OUTPUT+0x8>
   122b2:	00 
   122b3:	c6 05 4e 5d 29 00 00 	movb   $0x0,0x295d4e(%rip)        # 2a8008 <cellbox::executor::FRONTEND_OUTPUT>
   122ba:	74 0e                	je     122ca <cellbox::executor::relay_device+0x1aa>
   122bc:	0f b6 05 3d 5d 29 00 	movzbl 0x295d3d(%rip),%eax        # 2a8000 <cellbox::executor::STREAMS_MERGED>
   122c3:	84 c0                	test   %al,%al
   122c5:	0f 95 c0             	setne  %al
   122c8:	eb 02                	jmp    122cc <cellbox::executor::relay_device+0x1ac>
   122ca:	31 c0                	xor    %eax,%eax
   122cc:	40 80 f7 01          	xor    $0x1,%dil
   122d0:	40 08 c7             	or     %al,%dil
   122d3:	40 80 ff 01          	cmp    $0x1,%dil
   122d7:	75 2b                	jne    12304 <cellbox::executor::relay_device+0x1e4>
   122d9:	48 8b 84 24 80 00 00 	mov    0x80(%rsp),%rax
   122e0:	00 
   122e1:	48 89 44 24 20       	mov    %rax,0x20(%rsp)
   122e6:	48 8b 44 24 78       	mov    0x78(%rsp),%rax
   122eb:	48 89 44 24 18       	mov    %rax,0x18(%rsp)
   122f0:	48 8b 44 24 68       	mov    0x68(%rsp),%rax
   122f5:	48 8b 4c 24 70       	mov    0x70(%rsp),%rcx
   122fa:	48 89 4c 24 10       	mov    %rcx,0x10(%rsp)
   122ff:	48 89 44 24 08       	mov    %rax,0x8(%rsp)
   12304:	66 bd ff ff          	mov    $0xffff,%bp
   12308:	48 85 db             	test   %rbx,%rbx
   1230b:	0f 84 b0 00 00 00    	je     123c1 <cellbox::executor::relay_device+0x2a1>
   12311:	45 31 ff             	xor    %r15d,%r15d
   12314:	45 31 e4             	xor    %r12d,%r12d
   12317:	eb 2c                	jmp    12345 <cellbox::executor::relay_device+0x225>
   12319:	0f 1f 80 00 00 00 00 	nopl   0x0(%rax)
   12320:	b8 68 00 00 00       	mov    $0x68,%eax
   12325:	31 ff                	xor    %edi,%edi
   12327:	31 f6                	xor    %esi,%esi
   12329:	31 d2                	xor    %edx,%edx
   1232b:	45 31 d2             	xor    %r10d,%r10d
   1232e:	0f 05                	syscall
   12330:	41 ff c7             	inc    %r15d
   12333:	b8 ff ff ff ff       	mov    $0xffffffff,%eax
   12338:	44 0f 44 f8          	cmove  %eax,%r15d
   1233c:	49 39 dc             	cmp    %rbx,%r12
   1233f:	0f 83 7c 00 00 00    	jae    123c1 <cellbox::executor::relay_device+0x2a1>
   12345:	49 89 dd             	mov    %rbx,%r13
   12348:	4d 29 e5             	sub    %r12,%r13
   1234b:	4b 8d 14 26          	lea    (%r14,%r12,1),%rdx
   1234f:	48 8d bc 24 88 00 00 	lea    0x88(%rsp),%rdi
   12356:	00 
   12357:	48 8d 74 24 08       	lea    0x8(%rsp),%rsi
   1235c:	4c 89 e9             	mov    %r13,%rcx
   1235f:	45 31 c0             	xor    %r8d,%r8d
   12362:	e8 49 1f 19 00       	call   1a42b0 <<terminal_sdk::stream::Endpoint>::write>
   12367:	66 83 bc 24 88 00 00 	cmpw   $0x1,0x88(%rsp)
   1236e:	00 01 
   12370:	75 2e                	jne    123a0 <cellbox::executor::relay_device+0x280>
   12372:	0f b7 84 24 8a 00 00 	movzwl 0x8a(%rsp),%eax
   12379:	00 
   1237a:	66 83 f8 0e          	cmp    $0xe,%ax
   1237e:	75 3f                	jne    123bf <cellbox::executor::relay_device+0x29f>
   12380:	41 83 ff 08          	cmp    $0x8,%r15d
   12384:	72 9a                	jb     12320 <cellbox::executor::relay_device+0x200>
   12386:	b8 23 00 00 00       	mov    $0x23,%eax
   1238b:	bf 01 00 00 00       	mov    $0x1,%edi
   12390:	31 f6                	xor    %esi,%esi
   12392:	31 d2                	xor    %edx,%edx
   12394:	45 31 d2             	xor    %r10d,%r10d
   12397:	0f 05                	syscall
   12399:	eb 95                	jmp    12330 <cellbox::executor::relay_device+0x210>
   1239b:	0f 1f 44 00 00       	nopl   0x0(%rax,%rax,1)
   123a0:	48 8b 84 24 90 00 00 	mov    0x90(%rsp),%rax
   123a7:	00 
   123a8:	48 8d 48 ff          	lea    -0x1(%rax),%rcx
   123ac:	4c 39 e9             	cmp    %r13,%rcx
   123af:	73 0a                	jae    123bb <cellbox::executor::relay_device+0x29b>
   123b1:	49 01 c4             	add    %rax,%r12
   123b4:	49 39 dc             	cmp    %rbx,%r12
   123b7:	72 8c                	jb     12345 <cellbox::executor::relay_device+0x225>
   123b9:	eb 06                	jmp    123c1 <cellbox::executor::relay_device+0x2a1>
   123bb:	66 b8 03 00          	mov    $0x3,%ax
   123bf:	89 c5                	mov    %eax,%ebp
   123c1:	89 e8                	mov    %ebp,%eax
   123c3:	48 81 c4 98 00 00 00 	add    $0x98,%rsp
   123ca:	5b                   	pop    %rbx
   123cb:	41 5c                	pop    %r12
   123cd:	41 5d                	pop    %r13
   123cf:	41 5e                	pop    %r14
   123d1:	41 5f                	pop    %r15
   123d3:	5d                   	pop    %rbp
   123d4:	c3                   	ret
