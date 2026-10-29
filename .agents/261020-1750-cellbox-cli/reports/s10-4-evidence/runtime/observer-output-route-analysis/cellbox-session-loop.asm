
/tmp/cellbox-s10-3d-resume/cand/desktop/files/cellbox:     file format elf64-x86-64


Disassembly of section .text:

0000000000094f70 <cellbox::run_session_loop>:
   94f70:	55                   	push   %rbp
   94f71:	41 57                	push   %r15
   94f73:	41 56                	push   %r14
   94f75:	41 55                	push   %r13
   94f77:	41 54                	push   %r12
   94f79:	53                   	push   %rbx
   94f7a:	48 81 ec 00 10 00 00 	sub    $0x1000,%rsp
   94f81:	48 c7 04 24 00 00 00 	movq   $0x0,(%rsp)
   94f88:	00 
   94f89:	48 81 ec f8 00 00 00 	sub    $0xf8,%rsp
   94f90:	48 8d bc 24 f1 00 00 	lea    0xf1(%rsp),%rdi
   94f97:	00 
   94f98:	ba 00 10 00 00       	mov    $0x1000,%edx
   94f9d:	31 f6                	xor    %esi,%esi
   94f9f:	ff 15 bb 25 21 00    	call   *0x2125bb(%rip)        # 2a7560 <memchr::arch::x86_64::memchr::memchr_raw::FN+0x278>
   94fa5:	48 8d 7c 24 60       	lea    0x60(%rsp),%rdi
   94faa:	be 20 00 00 00       	mov    $0x20,%esi
   94faf:	e8 8c c3 fe ff       	call   81340 <<alloc::collections::vec_deque::VecDeque<alloc::string::String>>::with_capacity_in>
   94fb4:	48 8d ac 24 80 00 00 	lea    0x80(%rsp),%rbp
   94fbb:	00 
   94fbc:	be e8 03 00 00       	mov    $0x3e8,%esi
   94fc1:	48 89 ef             	mov    %rbp,%rdi
   94fc4:	e8 77 c3 fe ff       	call   81340 <<alloc::collections::vec_deque::VecDeque<alloc::string::String>>::with_capacity_in>
   94fc9:	c6 84 24 a0 00 00 00 	movb   $0x0,0xa0(%rsp)
   94fd0:	00 
   94fd1:	48 8d 05 0d e1 19 00 	lea    0x19e10d(%rip),%rax        # 2330e5 <unicode_width::tables::EMOJI_MODIFIER_LEAF_7+0x1911>
   94fd8:	48 89 84 24 c8 00 00 	mov    %rax,0xc8(%rsp)
   94fdf:	00 
   94fe0:	48 c7 84 24 d0 00 00 	movq   $0x9,0xd0(%rsp)
   94fe7:	00 09 00 00 00 
   94fec:	c6 84 24 f0 00 00 00 	movb   $0x0,0xf0(%rsp)
   94ff3:	00 
   94ff4:	48 c7 84 24 a8 00 00 	movq   $0x0,0xa8(%rsp)
   94ffb:	00 00 00 00 00 
   95000:	48 c7 84 24 b0 00 00 	movq   $0x8,0xb0(%rsp)
   95007:	00 08 00 00 00 
   9500c:	48 c7 84 24 b8 00 00 	movq   $0x0,0xb8(%rsp)
   95013:	00 00 00 00 00 
   95018:	48 c7 84 24 c0 00 00 	movq   $0x1,0xc0(%rsp)
   9501f:	00 01 00 00 00 
   95024:	48 c7 84 24 d8 00 00 	movq   $0x0,0xd8(%rsp)
   9502b:	00 00 00 00 00 
   95030:	48 c7 84 24 e8 00 00 	movq   $0x0,0xe8(%rsp)
   95037:	00 00 00 00 00 
   9503c:	48 c7 44 24 38 00 00 	movq   $0x0,0x38(%rsp)
   95043:	00 00 
   95045:	48 c7 44 24 40 08 00 	movq   $0x8,0x40(%rsp)
   9504c:	00 00 
   9504e:	48 c7 44 24 48 00 00 	movq   $0x0,0x48(%rsp)
   95055:	00 00 
   95057:	48 8d 3d 4e e0 19 00 	lea    0x19e04e(%rip),%rdi        # 2330ac <unicode_width::tables::EMOJI_MODIFIER_LEAF_7+0x18d8>
   9505e:	be 39 00 00 00       	mov    $0x39,%esi
   95063:	31 d2                	xor    %edx,%edx
   95065:	e8 76 2b f8 ff       	call   17be0 <cellbox::executor::terminal_write>
   9506a:	66 83 f8 ff          	cmp    $0xffff,%ax
   9506e:	0f 84 0e 04 00 00    	je     95482 <cellbox::run_session_loop+0x512>
   95074:	66 89 44 24 0e       	mov    %ax,0xe(%rsp)
   95079:	4c 8b 74 24 40       	mov    0x40(%rsp),%r14
   9507e:	48 8b 44 24 38       	mov    0x38(%rsp),%rax
   95083:	48 85 c0             	test   %rax,%rax
   95086:	74 15                	je     9509d <cellbox::run_session_loop+0x12d>
   95088:	48 c1 e0 03          	shl    $0x3,%rax
   9508c:	48 8d 14 40          	lea    (%rax,%rax,2),%rdx
   95090:	be 08 00 00 00       	mov    $0x8,%esi
   95095:	4c 89 f7             	mov    %r14,%rdi
   95098:	e8 83 dc 04 00       	call   e2d20 <<linked_list_allocator::Heap>::deallocate>
   9509d:	48 8d 7c 24 60       	lea    0x60(%rsp),%rdi
   950a2:	e8 c9 9e ff ff       	call   8ef70 <<alloc::collections::vec_deque::VecDeque<alloc::string::String> as core::ops::drop::Drop>::drop>
   950a7:	48 8b 44 24 60       	mov    0x60(%rsp),%rax
   950ac:	48 85 c0             	test   %rax,%rax
   950af:	74 17                	je     950c8 <cellbox::run_session_loop+0x158>
   950b1:	48 8b 7c 24 68       	mov    0x68(%rsp),%rdi
   950b6:	48 c1 e0 03          	shl    $0x3,%rax
   950ba:	48 8d 14 40          	lea    (%rax,%rax,2),%rdx
   950be:	be 08 00 00 00       	mov    $0x8,%esi
   950c3:	e8 58 dc 04 00       	call   e2d20 <<linked_list_allocator::Heap>::deallocate>
   950c8:	48 89 ef             	mov    %rbp,%rdi
   950cb:	e8 a0 9e ff ff       	call   8ef70 <<alloc::collections::vec_deque::VecDeque<alloc::string::String> as core::ops::drop::Drop>::drop>
   950d0:	48 8b 84 24 80 00 00 	mov    0x80(%rsp),%rax
   950d7:	00 
   950d8:	48 85 c0             	test   %rax,%rax
   950db:	0f b7 5c 24 0e       	movzwl 0xe(%rsp),%ebx
   950e0:	74 1a                	je     950fc <cellbox::run_session_loop+0x18c>
   950e2:	48 8b bc 24 88 00 00 	mov    0x88(%rsp),%rdi
   950e9:	00 
   950ea:	48 c1 e0 03          	shl    $0x3,%rax
   950ee:	48 8d 14 40          	lea    (%rax,%rax,2),%rdx
   950f2:	be 08 00 00 00       	mov    $0x8,%esi
   950f7:	e8 24 dc 04 00       	call   e2d20 <<linked_list_allocator::Heap>::deallocate>
   950fc:	48 8b bc 24 b0 00 00 	mov    0xb0(%rsp),%rdi
   95103:	00 
   95104:	48 8b b4 24 b8 00 00 	mov    0xb8(%rsp),%rsi
   9510b:	00 
   9510c:	e8 9f 91 fa ff       	call   3e2b0 <<alloc::vec::Vec<cellbox::jobs::Job> as core::ops::drop::Drop>::drop>
   95111:	48 8b 84 24 a8 00 00 	mov    0xa8(%rsp),%rax
   95118:	00 
   95119:	48 85 c0             	test   %rax,%rax
   9511c:	74 1a                	je     95138 <cellbox::run_session_loop+0x1c8>
   9511e:	48 8b bc 24 b0 00 00 	mov    0xb0(%rsp),%rdi
   95125:	00 
   95126:	48 c1 e0 03          	shl    $0x3,%rax
   9512a:	48 8d 14 c0          	lea    (%rax,%rax,8),%rdx
   9512e:	be 08 00 00 00       	mov    $0x8,%esi
   95133:	e8 e8 db 04 00       	call   e2d20 <<linked_list_allocator::Heap>::deallocate>
   95138:	48 8b bc 24 d8 00 00 	mov    0xd8(%rsp),%rdi
   9513f:	00 
   95140:	48 85 ff             	test   %rdi,%rdi
   95143:	0f 84 25 03 00 00    	je     9546e <cellbox::run_session_loop+0x4fe>
   95149:	4c 8b a4 24 e0 00 00 	mov    0xe0(%rsp),%r12
   95150:	00 
   95151:	4c 8b b4 24 e8 00 00 	mov    0xe8(%rsp),%r14
   95158:	00 
   95159:	4d 85 f6             	test   %r14,%r14
   9515c:	0f 84 27 02 00 00    	je     95389 <cellbox::run_session_loop+0x419>
   95162:	41 bf 20 02 00 00    	mov    $0x220,%r15d
   95168:	48 89 f8             	mov    %rdi,%rax
   9516b:	31 ff                	xor    %edi,%edi
   9516d:	eb 0f                	jmp    9517e <cellbox::run_session_loop+0x20e>
   9516f:	90                   	nop
   95170:	b8 00 00 00 00       	mov    $0x0,%eax
   95175:	49 ff ce             	dec    %r14
   95178:	0f 84 04 02 00 00    	je     95382 <cellbox::run_session_loop+0x412>
   9517e:	48 85 ff             	test   %rdi,%rdi
   95181:	0f 85 8c 00 00 00    	jne    95213 <cellbox::run_session_loop+0x2a3>
   95187:	4d 85 e4             	test   %r12,%r12
   9518a:	74 31                	je     951bd <cellbox::run_session_loop+0x24d>
   9518c:	4c 89 e1             	mov    %r12,%rcx
   9518f:	48 89 c7             	mov    %rax,%rdi
   95192:	48 83 e1 07          	and    $0x7,%rcx
   95196:	74 2a                	je     951c2 <cellbox::run_session_loop+0x252>
   95198:	31 d2                	xor    %edx,%edx
   9519a:	66 0f 1f 44 00 00    	nopw   0x0(%rax,%rax,1)
   951a0:	48 8b bf 20 02 00 00 	mov    0x220(%rdi),%rdi
   951a7:	48 ff c2             	inc    %rdx
   951aa:	48 39 d1             	cmp    %rdx,%rcx
   951ad:	75 f1                	jne    951a0 <cellbox::run_session_loop+0x230>
   951af:	4c 89 e0             	mov    %r12,%rax
   951b2:	48 29 d0             	sub    %rdx,%rax
   951b5:	49 83 fc 08          	cmp    $0x8,%r12
   951b9:	73 15                	jae    951d0 <cellbox::run_session_loop+0x260>
   951bb:	eb 51                	jmp    9520e <cellbox::run_session_loop+0x29e>
   951bd:	48 89 c7             	mov    %rax,%rdi
   951c0:	eb 4c                	jmp    9520e <cellbox::run_session_loop+0x29e>
   951c2:	4c 89 e0             	mov    %r12,%rax
   951c5:	49 83 fc 08          	cmp    $0x8,%r12
   951c9:	72 43                	jb     9520e <cellbox::run_session_loop+0x29e>
   951cb:	0f 1f 44 00 00       	nopl   0x0(%rax,%rax,1)
   951d0:	48 8b 8f 20 02 00 00 	mov    0x220(%rdi),%rcx
   951d7:	48 8b 89 20 02 00 00 	mov    0x220(%rcx),%rcx
   951de:	48 8b 89 20 02 00 00 	mov    0x220(%rcx),%rcx
   951e5:	48 8b 89 20 02 00 00 	mov    0x220(%rcx),%rcx
   951ec:	48 8b 89 20 02 00 00 	mov    0x220(%rcx),%rcx
   951f3:	48 8b 89 20 02 00 00 	mov    0x220(%rcx),%rcx
   951fa:	48 8b 89 20 02 00 00 	mov    0x220(%rcx),%rcx
   95201:	48 8b b9 20 02 00 00 	mov    0x220(%rcx),%rdi
   95208:	48 83 c0 f8          	add    $0xfffffffffffffff8,%rax
   9520c:	75 c2                	jne    951d0 <cellbox::run_session_loop+0x260>
   9520e:	31 c0                	xor    %eax,%eax
   95210:	45 31 e4             	xor    %r12d,%r12d
   95213:	0f b7 8f 1a 02 00 00 	movzwl 0x21a(%rdi),%ecx
   9521a:	49 39 cc             	cmp    %rcx,%r12
   9521d:	73 11                	jae    95230 <cellbox::run_session_loop+0x2c0>
   9521f:	4c 89 e5             	mov    %r12,%rbp
   95222:	49 89 c4             	mov    %rax,%r12
   95225:	49 89 fd             	mov    %rdi,%r13
   95228:	eb 43                	jmp    9526d <cellbox::run_session_loop+0x2fd>
   9522a:	66 0f 1f 44 00 00    	nopw   0x0(%rax,%rax,1)
   95230:	4c 8b 2f             	mov    (%rdi),%r13
   95233:	4d 85 ed             	test   %r13,%r13
   95236:	0f 84 71 08 00 00    	je     95aad <cellbox::run_session_loop+0xb3d>
   9523c:	4c 8d 60 01          	lea    0x1(%rax),%r12
   95240:	0f b7 af 18 02 00 00 	movzwl 0x218(%rdi),%ebp
   95247:	48 85 c0             	test   %rax,%rax
   9524a:	ba 80 02 00 00       	mov    $0x280,%edx
   9524f:	49 0f 44 d7          	cmove  %r15,%rdx
   95253:	be 08 00 00 00       	mov    $0x8,%esi
   95258:	e8 c3 da 04 00       	call   e2d20 <<linked_list_allocator::Heap>::deallocate>
   9525d:	4c 89 ef             	mov    %r13,%rdi
   95260:	4c 89 e0             	mov    %r12,%rax
   95263:	66 41 3b ad 1a 02 00 	cmp    0x21a(%r13),%bp
   9526a:	00 
   9526b:	73 c3                	jae    95230 <cellbox::run_session_loop+0x2c0>
   9526d:	4d 85 e4             	test   %r12,%r12
   95270:	0f 84 8a 00 00 00    	je     95300 <cellbox::run_session_loop+0x390>
   95276:	48 8d 04 ed 28 02 00 	lea    0x228(,%rbp,8),%rax
   9527d:	00 
   9527e:	4c 01 e8             	add    %r13,%rax
   95281:	4c 89 e1             	mov    %r12,%rcx
   95284:	48 83 e1 07          	and    $0x7,%rcx
   95288:	0f 84 e2 00 00 00    	je     95370 <cellbox::run_session_loop+0x400>
   9528e:	31 d2                	xor    %edx,%edx
   95290:	48 8b 38             	mov    (%rax),%rdi
   95293:	48 8d 87 20 02 00 00 	lea    0x220(%rdi),%rax
   9529a:	48 ff c2             	inc    %rdx
   9529d:	48 39 d1             	cmp    %rdx,%rcx
   952a0:	75 ee                	jne    95290 <cellbox::run_session_loop+0x320>
   952a2:	4c 89 e1             	mov    %r12,%rcx
   952a5:	48 29 d1             	sub    %rdx,%rcx
   952a8:	49 83 fc 08          	cmp    $0x8,%r12
   952ac:	72 43                	jb     952f1 <cellbox::run_session_loop+0x381>
   952ae:	66 90                	xchg   %ax,%ax
   952b0:	48 8b 00             	mov    (%rax),%rax
   952b3:	48 8b 80 20 02 00 00 	mov    0x220(%rax),%rax
   952ba:	48 8b 80 20 02 00 00 	mov    0x220(%rax),%rax
   952c1:	48 8b 80 20 02 00 00 	mov    0x220(%rax),%rax
   952c8:	48 8b 80 20 02 00 00 	mov    0x220(%rax),%rax
   952cf:	48 8b 80 20 02 00 00 	mov    0x220(%rax),%rax
   952d6:	48 8b 80 20 02 00 00 	mov    0x220(%rax),%rax
   952dd:	48 8b b8 20 02 00 00 	mov    0x220(%rax),%rdi
   952e4:	48 8d 87 20 02 00 00 	lea    0x220(%rdi),%rax
   952eb:	48 83 c1 f8          	add    $0xfffffffffffffff8,%rcx
   952ef:	75 bf                	jne    952b0 <cellbox::run_session_loop+0x340>
   952f1:	45 31 e4             	xor    %r12d,%r12d
   952f4:	eb 11                	jmp    95307 <cellbox::run_session_loop+0x397>
   952f6:	66 2e 0f 1f 84 00 00 	cs nopw 0x0(%rax,%rax,1)
   952fd:	00 00 00 
   95300:	4c 8d 65 01          	lea    0x1(%rbp),%r12
   95304:	4c 89 ef             	mov    %r13,%rdi
   95307:	48 8d 6c 6d 00       	lea    0x0(%rbp,%rbp,2),%rbp
   9530c:	49 8b 54 ed 08       	mov    0x8(%r13,%rbp,8),%rdx
   95311:	48 85 d2             	test   %rdx,%rdx
   95314:	74 22                	je     95338 <cellbox::run_session_loop+0x3c8>
   95316:	48 8d 04 ed 08 00 00 	lea    0x8(,%rbp,8),%rax
   9531d:	00 
   9531e:	4c 01 e8             	add    %r13,%rax
   95321:	48 8b 40 08          	mov    0x8(%rax),%rax
   95325:	be 01 00 00 00       	mov    $0x1,%esi
   9532a:	48 89 fb             	mov    %rdi,%rbx
   9532d:	48 89 c7             	mov    %rax,%rdi
   95330:	e8 eb d9 04 00       	call   e2d20 <<linked_list_allocator::Heap>::deallocate>
   95335:	48 89 df             	mov    %rbx,%rdi
   95338:	48 8d 04 ed 10 01 00 	lea    0x110(,%rbp,8),%rax
   9533f:	00 
   95340:	4c 01 e8             	add    %r13,%rax
   95343:	48 8b 10             	mov    (%rax),%rdx
   95346:	48 85 d2             	test   %rdx,%rdx
   95349:	0f 84 21 fe ff ff    	je     95170 <cellbox::run_session_loop+0x200>
   9534f:	48 8b 40 08          	mov    0x8(%rax),%rax
   95353:	be 01 00 00 00       	mov    $0x1,%esi
   95358:	48 89 fb             	mov    %rdi,%rbx
   9535b:	48 89 c7             	mov    %rax,%rdi
   9535e:	e8 bd d9 04 00       	call   e2d20 <<linked_list_allocator::Heap>::deallocate>
   95363:	48 89 df             	mov    %rbx,%rdi
   95366:	e9 05 fe ff ff       	jmp    95170 <cellbox::run_session_loop+0x200>
   9536b:	0f 1f 44 00 00       	nopl   0x0(%rax,%rax,1)
   95370:	4c 89 e1             	mov    %r12,%rcx
   95373:	49 83 fc 08          	cmp    $0x8,%r12
   95377:	0f 82 74 ff ff ff    	jb     952f1 <cellbox::run_session_loop+0x381>
   9537d:	e9 2e ff ff ff       	jmp    952b0 <cellbox::run_session_loop+0x340>
   95382:	48 85 ff             	test   %rdi,%rdi
   95385:	75 77                	jne    953fe <cellbox::run_session_loop+0x48e>
   95387:	31 ff                	xor    %edi,%edi
   95389:	4d 85 e4             	test   %r12,%r12
   9538c:	74 70                	je     953fe <cellbox::run_session_loop+0x48e>
   9538e:	4c 89 e0             	mov    %r12,%rax
   95391:	48 83 e0 07          	and    $0x7,%rax
   95395:	0f 84 4c 06 00 00    	je     959e7 <cellbox::run_session_loop+0xa77>
   9539b:	31 c9                	xor    %ecx,%ecx
   9539d:	0f 1f 00             	nopl   (%rax)
   953a0:	48 8b bf 20 02 00 00 	mov    0x220(%rdi),%rdi
   953a7:	48 ff c1             	inc    %rcx
   953aa:	48 39 c8             	cmp    %rcx,%rax
   953ad:	75 f1                	jne    953a0 <cellbox::run_session_loop+0x430>
   953af:	4c 89 e0             	mov    %r12,%rax
   953b2:	48 29 c8             	sub    %rcx,%rax
   953b5:	49 83 fc 08          	cmp    $0x8,%r12
   953b9:	72 43                	jb     953fe <cellbox::run_session_loop+0x48e>
   953bb:	0f 1f 44 00 00       	nopl   0x0(%rax,%rax,1)
   953c0:	48 8b 8f 20 02 00 00 	mov    0x220(%rdi),%rcx
   953c7:	48 8b 89 20 02 00 00 	mov    0x220(%rcx),%rcx
   953ce:	48 8b 89 20 02 00 00 	mov    0x220(%rcx),%rcx
   953d5:	48 8b 89 20 02 00 00 	mov    0x220(%rcx),%rcx
   953dc:	48 8b 89 20 02 00 00 	mov    0x220(%rcx),%rcx
   953e3:	48 8b 89 20 02 00 00 	mov    0x220(%rcx),%rcx
   953ea:	48 8b 89 20 02 00 00 	mov    0x220(%rcx),%rcx
   953f1:	48 8b b9 20 02 00 00 	mov    0x220(%rcx),%rdi
   953f8:	48 83 c0 f8          	add    $0xfffffffffffffff8,%rax
   953fc:	75 c2                	jne    953c0 <cellbox::run_session_loop+0x450>
   953fe:	48 8b 0f             	mov    (%rdi),%rcx
   95401:	45 31 f6             	xor    %r14d,%r14d
   95404:	48 85 c9             	test   %rcx,%rcx
   95407:	74 45                	je     9544e <cellbox::run_session_loop+0x4de>
   95409:	41 bf 20 02 00 00    	mov    $0x220,%r15d
   9540f:	48 89 f8             	mov    %rdi,%rax
   95412:	66 66 66 66 66 2e 0f 	data16 data16 data16 data16 cs nopw 0x0(%rax,%rax,1)
   95419:	1f 84 00 00 00 00 00 
   95420:	48 89 cb             	mov    %rcx,%rbx
   95423:	4d 85 f6             	test   %r14,%r14
   95426:	4d 8d 76 01          	lea    0x1(%r14),%r14
   9542a:	ba 80 02 00 00       	mov    $0x280,%edx
   9542f:	49 0f 44 d7          	cmove  %r15,%rdx
   95433:	be 08 00 00 00       	mov    $0x8,%esi
   95438:	48 89 c7             	mov    %rax,%rdi
   9543b:	e8 e0 d8 04 00       	call   e2d20 <<linked_list_allocator::Heap>::deallocate>
   95440:	48 89 df             	mov    %rbx,%rdi
   95443:	48 8b 0b             	mov    (%rbx),%rcx
   95446:	48 89 d8             	mov    %rbx,%rax
   95449:	48 85 c9             	test   %rcx,%rcx
   9544c:	75 d2                	jne    95420 <cellbox::run_session_loop+0x4b0>
   9544e:	4d 85 f6             	test   %r14,%r14
   95451:	b8 20 02 00 00       	mov    $0x220,%eax
   95456:	ba 80 02 00 00       	mov    $0x280,%edx
   9545b:	48 0f 44 d0          	cmove  %rax,%rdx
   9545f:	be 08 00 00 00       	mov    $0x8,%esi
   95464:	e8 b7 d8 04 00       	call   e2d20 <<linked_list_allocator::Heap>::deallocate>
   95469:	0f b7 5c 24 0e       	movzwl 0xe(%rsp),%ebx
   9546e:	89 d8                	mov    %ebx,%eax
   95470:	48 81 c4 f8 10 00 00 	add    $0x10f8,%rsp
   95477:	5b                   	pop    %rbx
   95478:	41 5c                	pop    %r12
   9547a:	41 5d                	pop    %r13
   9547c:	41 5e                	pop    %r14
   9547e:	41 5f                	pop    %r15
   95480:	5d                   	pop    %rbp
   95481:	c3                   	ret
   95482:	48 8d 05 6e 2c 81 00 	lea    0x812c6e(%rip),%rax        # 8a80f7 <ostd::heap::HEAP_MEM+0x7>
   95489:	48 83 e0 f0          	and    $0xfffffffffffffff0,%rax
   9548d:	48 8d 0d 5c 2c 91 00 	lea    0x912c5c(%rip),%rcx        # 9a80f0 <ostd::heap::HEAP_INIT>
   95494:	48 89 44 24 58       	mov    %rax,0x58(%rsp)
   95499:	48 29 c1             	sub    %rax,%rcx
   9549c:	48 89 4c 24 50       	mov    %rcx,0x50(%rsp)
   954a1:	4c 8d 35 3d dc 19 00 	lea    0x19dc3d(%rip),%r14        # 2330e5 <unicode_width::tables::EMOJI_MODIFIER_LEAF_7+0x1911>
   954a8:	40 b5 01             	mov    $0x1,%bpl
   954ab:	0f 1f 44 00 00       	nopl   0x0(%rax,%rax,1)
   954b0:	be 09 00 00 00       	mov    $0x9,%esi
   954b5:	4c 89 f7             	mov    %r14,%rdi
   954b8:	31 d2                	xor    %edx,%edx
   954ba:	e8 21 27 f8 ff       	call   17be0 <cellbox::executor::terminal_write>
   954bf:	66 83 f8 ff          	cmp    $0xffff,%ax
   954c3:	0f 85 cb 04 00 00    	jne    95994 <cellbox::run_session_loop+0xa24>
   954c9:	48 8d 7c 24 18       	lea    0x18(%rsp),%rdi
   954ce:	48 8d 74 24 38       	lea    0x38(%rsp),%rsi
   954d3:	e8 e8 49 f8 ff       	call   19ec0 <cellbox::executor::terminal_read_line>
   954d8:	4c 8b 64 24 18       	mov    0x18(%rsp),%r12
   954dd:	0f b7 44 24 20       	movzwl 0x20(%rsp),%eax
   954e2:	49 83 fc fe          	cmp    $0xfffffffffffffffe,%r12
   954e6:	0f 84 a8 04 00 00    	je     95994 <cellbox::run_session_loop+0xa24>
   954ec:	49 83 fc ff          	cmp    $0xffffffffffffffff,%r12
   954f0:	0f 84 99 04 00 00    	je     9598f <cellbox::run_session_loop+0xa1f>
   954f6:	0f b7 c0             	movzwl %ax,%eax
   954f9:	8b 4c 24 22          	mov    0x22(%rsp),%ecx
   954fd:	44 0f b7 6c 24 26    	movzwl 0x26(%rsp),%r13d
   95503:	49 c1 e5 20          	shl    $0x20,%r13
   95507:	49 09 cd             	or     %rcx,%r13
   9550a:	48 8b 5c 24 28       	mov    0x28(%rsp),%rbx
   9550f:	49 c1 e5 10          	shl    $0x10,%r13
   95513:	49 09 c5             	or     %rax,%r13
   95516:	4c 89 ef             	mov    %r13,%rdi
   95519:	48 89 de             	mov    %rbx,%rsi
   9551c:	e8 4f 6b 13 00       	call   1cc070 <<str>::trim_matches::<<char>::is_whitespace>>
   95521:	48 85 d2             	test   %rdx,%rdx
   95524:	74 05                	je     9552b <cellbox::run_session_loop+0x5bb>
   95526:	80 38 23             	cmpb   $0x23,(%rax)
   95529:	75 25                	jne    95550 <cellbox::run_session_loop+0x5e0>
   9552b:	4d 85 e4             	test   %r12,%r12
   9552e:	0f 84 88 02 00 00    	je     957bc <cellbox::run_session_loop+0x84c>
   95534:	44 89 e0             	mov    %r12d,%eax
   95537:	83 e0 07             	and    $0x7,%eax
   9553a:	49 83 fc 08          	cmp    $0x8,%r12
   9553e:	0f 83 09 02 00 00    	jae    9574d <cellbox::run_session_loop+0x7dd>
   95544:	31 c9                	xor    %ecx,%ecx
   95546:	e9 53 02 00 00       	jmp    9579e <cellbox::run_session_loop+0x82e>
   9554b:	0f 1f 44 00 00       	nopl   0x0(%rax,%rax,1)
   95550:	4c 89 ef             	mov    %r13,%rdi
   95553:	48 89 de             	mov    %rbx,%rsi
   95556:	e8 15 6b 13 00       	call   1cc070 <<str>::trim_matches::<<char>::is_whitespace>>
   9555b:	48 89 c6             	mov    %rax,%rsi
   9555e:	66 90                	xchg   %ax,%ax
   95560:	31 c0                	xor    %eax,%eax
   95562:	f0 40 0f b0 2d c5 df 	lock cmpxchg %bpl,0x1edfc5(%rip)        # 283530 <cellbox::history::GLOBAL_HISTORY>
   95569:	1e 00 
   9556b:	75 05                	jne    95572 <cellbox::run_session_loop+0x602>
   9556d:	eb 11                	jmp    95580 <cellbox::run_session_loop+0x610>
   9556f:	90                   	nop
   95570:	f3 90                	pause
   95572:	0f b6 05 b7 df 1e 00 	movzbl 0x1edfb7(%rip),%eax        # 283530 <cellbox::history::GLOBAL_HISTORY>
   95579:	84 c0                	test   %al,%al
   9557b:	75 f3                	jne    95570 <cellbox::run_session_loop+0x600>
   9557d:	eb e1                	jmp    95560 <cellbox::run_session_loop+0x5f0>
   9557f:	90                   	nop
   95580:	48 8d 3d b1 df 1e 00 	lea    0x1edfb1(%rip),%rdi        # 283538 <cellbox::history::GLOBAL_HISTORY+0x8>
   95587:	e8 54 82 01 00       	call   ad7e0 <<cellbox::history::History>::push>
   9558c:	c6 05 9d df 1e 00 00 	movb   $0x0,0x1edf9d(%rip)        # 283530 <cellbox::history::GLOBAL_HISTORY>
   95593:	66 66 66 66 2e 0f 1f 	data16 data16 data16 cs nopw 0x0(%rax,%rax,1)
   9559a:	84 00 00 00 00 00 
   955a0:	31 c0                	xor    %eax,%eax
   955a2:	f0 40 0f b0 2d 75 e4 	lock cmpxchg %bpl,0x1ee475(%rip)        # 283a20 <cellbox::shell_state::SINK>
   955a9:	1e 00 
   955ab:	75 05                	jne    955b2 <cellbox::run_session_loop+0x642>
   955ad:	eb 11                	jmp    955c0 <cellbox::run_session_loop+0x650>
   955af:	90                   	nop
   955b0:	f3 90                	pause
   955b2:	0f b6 05 67 e4 1e 00 	movzbl 0x1ee467(%rip),%eax        # 283a20 <cellbox::shell_state::SINK>
   955b9:	84 c0                	test   %al,%al
   955bb:	75 f3                	jne    955b0 <cellbox::run_session_loop+0x640>
   955bd:	eb e1                	jmp    955a0 <cellbox::run_session_loop+0x630>
   955bf:	90                   	nop
   955c0:	4c 8b 35 71 e4 1e 00 	mov    0x1ee471(%rip),%r14        # 283a38 <cellbox::shell_state::SINK+0x18>
   955c7:	4c 3b 35 5a e4 1e 00 	cmp    0x1ee45a(%rip),%r14        # 283a28 <cellbox::shell_state::SINK+0x8>
   955ce:	0f 84 aa 03 00 00    	je     9597e <cellbox::run_session_loop+0xa0e>
   955d4:	48 8b 05 55 e4 1e 00 	mov    0x1ee455(%rip),%rax        # 283a30 <cellbox::shell_state::SINK+0x10>
   955db:	4b 8d 0c 76          	lea    (%r14,%r14,2),%rcx
   955df:	48 c7 04 c8 00 00 00 	movq   $0x0,(%rax,%rcx,8)
   955e6:	00 
   955e7:	48 c7 44 c8 08 01 00 	movq   $0x1,0x8(%rax,%rcx,8)
   955ee:	00 00 
   955f0:	48 c7 44 c8 10 00 00 	movq   $0x0,0x10(%rax,%rcx,8)
   955f7:	00 00 
   955f9:	49 ff c6             	inc    %r14
   955fc:	4c 89 35 35 e4 1e 00 	mov    %r14,0x1ee435(%rip)        # 283a38 <cellbox::shell_state::SINK+0x18>
   95603:	c6 05 16 e4 1e 00 00 	movb   $0x0,0x1ee416(%rip)        # 283a20 <cellbox::shell_state::SINK>
   9560a:	48 8d 44 24 60       	lea    0x60(%rsp),%rax
   9560f:	48 89 44 24 18       	mov    %rax,0x18(%rsp)
   95614:	4c 89 6c 24 20       	mov    %r13,0x20(%rsp)
   95619:	48 89 5c 24 28       	mov    %rbx,0x28(%rsp)
   9561e:	c6 44 24 30 00       	movb   $0x0,0x30(%rsp)
   95623:	80 3d c6 2a 91 00 00 	cmpb   $0x0,0x912ac6(%rip)        # 9a80f0 <ostd::heap::HEAP_INIT>
   9562a:	75 5e                	jne    9568a <cellbox::run_session_loop+0x71a>
   9562c:	48 c7 05 89 2a 81 00 	movq   $0x0,0x812a89(%rip)        # 8a80c0 <ostd::heap::HEAP>
   95633:	00 00 00 00 
   95637:	48 8b 44 24 58       	mov    0x58(%rsp),%rax
   9563c:	48 8d 0d ad 2a 91 00 	lea    0x912aad(%rip),%rcx        # 9a80f0 <ostd::heap::HEAP_INIT>
   95643:	48 39 c8             	cmp    %rcx,%rax
   95646:	0f 84 50 04 00 00    	je     95a9c <cellbox::run_session_loop+0xb2c>
   9564c:	48 8b 54 24 50       	mov    0x50(%rsp),%rdx
   95651:	48 89 10             	mov    %rdx,(%rax)
   95654:	48 c7 40 08 00 00 00 	movq   $0x0,0x8(%rax)
   9565b:	00 
   9565c:	48 c7 05 61 2a 81 00 	movq   $0x0,0x812a61(%rip)        # 8a80c8 <ostd::heap::HEAP+0x8>
   95663:	00 00 00 00 
   95667:	48 89 05 62 2a 81 00 	mov    %rax,0x812a62(%rip)        # 8a80d0 <ostd::heap::HEAP+0x10>
   9566e:	48 89 05 63 2a 81 00 	mov    %rax,0x812a63(%rip)        # 8a80d8 <ostd::heap::HEAP+0x18>
   95675:	48 89 0d 64 2a 81 00 	mov    %rcx,0x812a64(%rip)        # 8a80e0 <ostd::heap::HEAP+0x20>
   9567c:	c6 05 65 2a 81 00 00 	movb   $0x0,0x812a65(%rip)        # 8a80e8 <ostd::heap::HEAP+0x28>
   95683:	c6 05 66 2a 91 00 01 	movb   $0x1,0x912a66(%rip)        # 9a80f0 <ostd::heap::HEAP_INIT>
   9568a:	bf 08 00 00 00       	mov    $0x8,%edi
   9568f:	be 18 00 00 00       	mov    $0x18,%esi
   95694:	e8 c7 d8 04 00       	call   e2f60 <<linked_list_allocator::Heap>::allocate_first_fit>
   95699:	48 85 c0             	test   %rax,%rax
   9569c:	0f 84 f0 03 00 00    	je     95a92 <cellbox::run_session_loop+0xb22>
   956a2:	48 89 c3             	mov    %rax,%rbx
   956a5:	48 c7 00 01 00 00 00 	movq   $0x1,(%rax)
   956ac:	48 c7 40 08 01 00 00 	movq   $0x1,0x8(%rax)
   956b3:	00 
   956b4:	c6 40 10 00          	movb   $0x0,0x10(%rax)
   956b8:	f0 48 ff 00          	lock incq (%rax)
   956bc:	0f 8e 12 04 00 00    	jle    95ad4 <cellbox::run_session_loop+0xb64>
   956c2:	c6 43 10 00          	movb   $0x0,0x10(%rbx)
   956c6:	48 8d 7c 24 18       	lea    0x18(%rsp),%rdi
   956cb:	e8 20 8b 02 00       	call   be1f0 <<cellbox::shell::ViShell>::dispatch::{closure#0}>
   956d0:	f0 48 ff 0b          	lock decq (%rbx)
   956d4:	75 08                	jne    956de <cellbox::run_session_loop+0x76e>
   956d6:	48 89 df             	mov    %rbx,%rdi
   956d9:	e8 02 4b 05 00       	call   ea1e0 <<alloc::sync::Arc<ostd::executor::ExecutorState>>::drop_slow>
   956de:	f0 48 ff 0b          	lock decq (%rbx)
   956e2:	75 0c                	jne    956f0 <cellbox::run_session_loop+0x780>
   956e4:	48 89 df             	mov    %rbx,%rdi
   956e7:	e8 f4 4a 05 00       	call   ea1e0 <<alloc::sync::Arc<ostd::executor::ExecutorState>>::drop_slow>
   956ec:	0f 1f 40 00          	nopl   0x0(%rax)
   956f0:	31 c0                	xor    %eax,%eax
   956f2:	f0 40 0f b0 2d 25 e3 	lock cmpxchg %bpl,0x1ee325(%rip)        # 283a20 <cellbox::shell_state::SINK>
   956f9:	1e 00 
   956fb:	74 13                	je     95710 <cellbox::run_session_loop+0x7a0>
   956fd:	0f 1f 00             	nopl   (%rax)
   95700:	0f b6 05 19 e3 1e 00 	movzbl 0x1ee319(%rip),%eax        # 283a20 <cellbox::shell_state::SINK>
   95707:	84 c0                	test   %al,%al
   95709:	74 e5                	je     956f0 <cellbox::run_session_loop+0x780>
   9570b:	f3 90                	pause
   9570d:	eb f1                	jmp    95700 <cellbox::run_session_loop+0x790>
   9570f:	90                   	nop
   95710:	48 8b 05 21 e3 1e 00 	mov    0x1ee321(%rip),%rax        # 283a38 <cellbox::shell_state::SINK+0x18>
   95717:	48 85 c0             	test   %rax,%rax
   9571a:	0f 84 a1 00 00 00    	je     957c1 <cellbox::run_session_loop+0x851>
   95720:	48 ff c8             	dec    %rax
   95723:	48 89 05 0e e3 1e 00 	mov    %rax,0x1ee30e(%rip)        # 283a38 <cellbox::shell_state::SINK+0x18>
   9572a:	48 8b 0d ff e2 1e 00 	mov    0x1ee2ff(%rip),%rcx        # 283a30 <cellbox::shell_state::SINK+0x10>
   95731:	48 8d 04 40          	lea    (%rax,%rax,2),%rax
   95735:	48 8b 14 c1          	mov    (%rcx,%rax,8),%rdx
   95739:	48 89 54 24 10       	mov    %rdx,0x10(%rsp)
   9573e:	4c 8b 7c c1 08       	mov    0x8(%rcx,%rax,8),%r15
   95743:	48 8b 74 c1 10       	mov    0x10(%rcx,%rax,8),%rsi
   95748:	e9 85 00 00 00       	jmp    957d2 <cellbox::run_session_loop+0x862>
   9574d:	4c 89 e2             	mov    %r12,%rdx
   95750:	48 83 e2 f8          	and    $0xfffffffffffffff8,%rdx
   95754:	31 c9                	xor    %ecx,%ecx
   95756:	66 2e 0f 1f 84 00 00 	cs nopw 0x0(%rax,%rax,1)
   9575d:	00 00 00 
   95760:	41 c6 44 0d 00 00    	movb   $0x0,0x0(%r13,%rcx,1)
   95766:	41 c6 44 0d 01 00    	movb   $0x0,0x1(%r13,%rcx,1)
   9576c:	41 c6 44 0d 02 00    	movb   $0x0,0x2(%r13,%rcx,1)
   95772:	41 c6 44 0d 03 00    	movb   $0x0,0x3(%r13,%rcx,1)
   95778:	41 c6 44 0d 04 00    	movb   $0x0,0x4(%r13,%rcx,1)
   9577e:	41 c6 44 0d 05 00    	movb   $0x0,0x5(%r13,%rcx,1)
   95784:	41 c6 44 0d 06 00    	movb   $0x0,0x6(%r13,%rcx,1)
   9578a:	41 c6 44 0d 07 00    	movb   $0x0,0x7(%r13,%rcx,1)
   95790:	48 83 c1 08          	add    $0x8,%rcx
   95794:	48 39 ca             	cmp    %rcx,%rdx
   95797:	75 c7                	jne    95760 <cellbox::run_session_loop+0x7f0>
   95799:	48 85 c0             	test   %rax,%rax
   9579c:	74 1e                	je     957bc <cellbox::run_session_loop+0x84c>
   9579e:	4c 01 e9             	add    %r13,%rcx
   957a1:	31 d2                	xor    %edx,%edx
   957a3:	66 66 66 66 2e 0f 1f 	data16 data16 data16 cs nopw 0x0(%rax,%rax,1)
   957aa:	84 00 00 00 00 00 
   957b0:	c6 04 11 00          	movb   $0x0,(%rcx,%rdx,1)
   957b4:	48 ff c2             	inc    %rdx
   957b7:	48 39 d0             	cmp    %rdx,%rax
   957ba:	75 f4                	jne    957b0 <cellbox::run_session_loop+0x840>
   957bc:	e9 9f 01 00 00       	jmp    95960 <cellbox::run_session_loop+0x9f0>
   957c1:	41 bf 01 00 00 00    	mov    $0x1,%r15d
   957c7:	31 f6                	xor    %esi,%esi
   957c9:	48 c7 44 24 10 00 00 	movq   $0x0,0x10(%rsp)
   957d0:	00 00 
   957d2:	c6 05 47 e2 1e 00 00 	movb   $0x0,0x1ee247(%rip)        # 283a20 <cellbox::shell_state::SINK>
   957d9:	4c 89 ff             	mov    %r15,%rdi
   957dc:	31 d2                	xor    %edx,%edx
   957de:	e8 fd 23 f8 ff       	call   17be0 <cellbox::executor::terminal_write>
   957e3:	89 c3                	mov    %eax,%ebx
   957e5:	4d 85 e4             	test   %r12,%r12
   957e8:	48 8b 54 24 10       	mov    0x10(%rsp),%rdx
   957ed:	0f 84 79 00 00 00    	je     9586c <cellbox::run_session_loop+0x8fc>
   957f3:	44 89 e0             	mov    %r12d,%eax
   957f6:	83 e0 07             	and    $0x7,%eax
   957f9:	49 83 fc 08          	cmp    $0x8,%r12
   957fd:	73 04                	jae    95803 <cellbox::run_session_loop+0x893>
   957ff:	31 c9                	xor    %ecx,%ecx
   95801:	eb 4b                	jmp    9584e <cellbox::run_session_loop+0x8de>
   95803:	4c 89 e6             	mov    %r12,%rsi
   95806:	48 83 e6 f8          	and    $0xfffffffffffffff8,%rsi
   9580a:	31 c9                	xor    %ecx,%ecx
   9580c:	0f 1f 40 00          	nopl   0x0(%rax)
   95810:	41 c6 44 0d 00 00    	movb   $0x0,0x0(%r13,%rcx,1)
   95816:	41 c6 44 0d 01 00    	movb   $0x0,0x1(%r13,%rcx,1)
   9581c:	41 c6 44 0d 02 00    	movb   $0x0,0x2(%r13,%rcx,1)
   95822:	41 c6 44 0d 03 00    	movb   $0x0,0x3(%r13,%rcx,1)
   95828:	41 c6 44 0d 04 00    	movb   $0x0,0x4(%r13,%rcx,1)
   9582e:	41 c6 44 0d 05 00    	movb   $0x0,0x5(%r13,%rcx,1)
   95834:	41 c6 44 0d 06 00    	movb   $0x0,0x6(%r13,%rcx,1)
   9583a:	41 c6 44 0d 07 00    	movb   $0x0,0x7(%r13,%rcx,1)
   95840:	48 83 c1 08          	add    $0x8,%rcx
   95844:	48 39 ce             	cmp    %rcx,%rsi
   95847:	75 c7                	jne    95810 <cellbox::run_session_loop+0x8a0>
   95849:	48 85 c0             	test   %rax,%rax
   9584c:	74 1e                	je     9586c <cellbox::run_session_loop+0x8fc>
   9584e:	4c 01 e9             	add    %r13,%rcx
   95851:	31 f6                	xor    %esi,%esi
   95853:	66 66 66 66 2e 0f 1f 	data16 data16 data16 cs nopw 0x0(%rax,%rax,1)
   9585a:	84 00 00 00 00 00 
   95860:	c6 04 31 00          	movb   $0x0,(%rcx,%rsi,1)
   95864:	48 ff c6             	inc    %rsi
   95867:	48 39 f0             	cmp    %rsi,%rax
   9586a:	75 f4                	jne    95860 <cellbox::run_session_loop+0x8f0>
   9586c:	48 85 d2             	test   %rdx,%rdx
   9586f:	74 7b                	je     958ec <cellbox::run_session_loop+0x97c>
   95871:	89 d0                	mov    %edx,%eax
   95873:	83 e0 07             	and    $0x7,%eax
   95876:	48 83 fa 08          	cmp    $0x8,%rdx
   9587a:	73 04                	jae    95880 <cellbox::run_session_loop+0x910>
   9587c:	31 c9                	xor    %ecx,%ecx
   9587e:	eb 4d                	jmp    958cd <cellbox::run_session_loop+0x95d>
   95880:	48 89 d6             	mov    %rdx,%rsi
   95883:	48 83 e6 f8          	and    $0xfffffffffffffff8,%rsi
   95887:	31 c9                	xor    %ecx,%ecx
   95889:	0f 1f 80 00 00 00 00 	nopl   0x0(%rax)
   95890:	41 c6 04 0f 00       	movb   $0x0,(%r15,%rcx,1)
   95895:	41 c6 44 0f 01 00    	movb   $0x0,0x1(%r15,%rcx,1)
   9589b:	41 c6 44 0f 02 00    	movb   $0x0,0x2(%r15,%rcx,1)
   958a1:	41 c6 44 0f 03 00    	movb   $0x0,0x3(%r15,%rcx,1)
   958a7:	41 c6 44 0f 04 00    	movb   $0x0,0x4(%r15,%rcx,1)
   958ad:	41 c6 44 0f 05 00    	movb   $0x0,0x5(%r15,%rcx,1)
   958b3:	41 c6 44 0f 06 00    	movb   $0x0,0x6(%r15,%rcx,1)
   958b9:	41 c6 44 0f 07 00    	movb   $0x0,0x7(%r15,%rcx,1)
   958bf:	48 83 c1 08          	add    $0x8,%rcx
   958c3:	48 39 ce             	cmp    %rcx,%rsi
   958c6:	75 c8                	jne    95890 <cellbox::run_session_loop+0x920>
   958c8:	48 85 c0             	test   %rax,%rax
   958cb:	74 1f                	je     958ec <cellbox::run_session_loop+0x97c>
   958cd:	4c 01 f9             	add    %r15,%rcx
   958d0:	31 f6                	xor    %esi,%esi
   958d2:	66 66 66 66 66 2e 0f 	data16 data16 data16 data16 cs nopw 0x0(%rax,%rax,1)
   958d9:	1f 84 00 00 00 00 00 
   958e0:	c6 04 31 00          	movb   $0x0,(%rcx,%rsi,1)
   958e4:	48 ff c6             	inc    %rsi
   958e7:	48 39 f0             	cmp    %rsi,%rax
   958ea:	75 f4                	jne    958e0 <cellbox::run_session_loop+0x970>
   958ec:	66 83 fb ff          	cmp    $0xffff,%bx
   958f0:	74 1b                	je     9590d <cellbox::run_session_loop+0x99d>
   958f2:	e9 02 01 00 00       	jmp    959f9 <cellbox::run_session_loop+0xa89>
   958f7:	66 0f 1f 84 00 00 00 	nopw   0x0(%rax,%rax,1)
   958fe:	00 00 
   95900:	f3 90                	pause
   95902:	0f b6 05 8b 27 81 00 	movzbl 0x81278b(%rip),%eax        # 8a8094 <cellbox::shell_state::EXIT_REQUEST>
   95909:	84 c0                	test   %al,%al
   9590b:	75 f3                	jne    95900 <cellbox::run_session_loop+0x990>
   9590d:	31 c0                	xor    %eax,%eax
   9590f:	f0 40 0f b0 2d 7c 27 	lock cmpxchg %bpl,0x81277c(%rip)        # 8a8094 <cellbox::shell_state::EXIT_REQUEST>
   95916:	81 00 
   95918:	75 e8                	jne    95902 <cellbox::run_session_loop+0x992>
   9591a:	8b 05 78 27 81 00    	mov    0x812778(%rip),%eax        # 8a8098 <cellbox::shell_state::EXIT_REQUEST+0x4>
   95920:	4c 63 35 75 27 81 00 	movslq 0x812775(%rip),%r14        # 8a809c <cellbox::shell_state::EXIT_REQUEST+0x8>
   95927:	c7 05 67 27 81 00 00 	movl   $0x0,0x812767(%rip)        # 8a8098 <cellbox::shell_state::EXIT_REQUEST+0x4>
   9592e:	00 00 00 
   95931:	c6 05 5c 27 81 00 00 	movb   $0x0,0x81275c(%rip)        # 8a8094 <cellbox::shell_state::EXIT_REQUEST>
   95938:	83 f8 01             	cmp    $0x1,%eax
   9593b:	0f 84 de 00 00 00    	je     95a1f <cellbox::run_session_loop+0xaaf>
   95941:	48 85 d2             	test   %rdx,%rdx
   95944:	4c 8d 35 9a d7 19 00 	lea    0x19d79a(%rip),%r14        # 2330e5 <unicode_width::tables::EMOJI_MODIFIER_LEAF_7+0x1911>
   9594b:	74 13                	je     95960 <cellbox::run_session_loop+0x9f0>
   9594d:	be 01 00 00 00       	mov    $0x1,%esi
   95952:	4c 89 ff             	mov    %r15,%rdi
   95955:	e8 c6 d3 04 00       	call   e2d20 <<linked_list_allocator::Heap>::deallocate>
   9595a:	66 0f 1f 44 00 00    	nopw   0x0(%rax,%rax,1)
   95960:	4d 85 e4             	test   %r12,%r12
   95963:	0f 84 47 fb ff ff    	je     954b0 <cellbox::run_session_loop+0x540>
   95969:	be 01 00 00 00       	mov    $0x1,%esi
   9596e:	4c 89 ef             	mov    %r13,%rdi
   95971:	4c 89 e2             	mov    %r12,%rdx
   95974:	e8 a7 d3 04 00       	call   e2d20 <<linked_list_allocator::Heap>::deallocate>
   95979:	e9 32 fb ff ff       	jmp    954b0 <cellbox::run_session_loop+0x540>
   9597e:	48 8d 3d a3 e0 1e 00 	lea    0x1ee0a3(%rip),%rdi        # 283a28 <cellbox::shell_state::SINK+0x8>
   95985:	e8 e6 b9 f9 ff       	call   31370 <<alloc::raw_vec::RawVec<alloc::vec::Vec<u8>>>::grow_one>
   9598a:	e9 45 fc ff ff       	jmp    955d4 <cellbox::run_session_loop+0x664>
   9598f:	e8 ac cd f7 ff       	call   12740 <cellbox::executor::close_session>
   95994:	66 89 44 24 0e       	mov    %ax,0xe(%rsp)
   95999:	48 8d ac 24 80 00 00 	lea    0x80(%rsp),%rbp
   959a0:	00 
   959a1:	4c 8b 74 24 40       	mov    0x40(%rsp),%r14
   959a6:	4c 8b 7c 24 48       	mov    0x48(%rsp),%r15
   959ab:	4d 85 ff             	test   %r15,%r15
   959ae:	0f 84 ca f6 ff ff    	je     9507e <cellbox::run_session_loop+0x10e>
   959b4:	4d 8d 66 08          	lea    0x8(%r14),%r12
   959b8:	eb 13                	jmp    959cd <cellbox::run_session_loop+0xa5d>
   959ba:	66 0f 1f 44 00 00    	nopw   0x0(%rax,%rax,1)
   959c0:	49 83 c4 18          	add    $0x18,%r12
   959c4:	49 ff cf             	dec    %r15
   959c7:	0f 84 b1 f6 ff ff    	je     9507e <cellbox::run_session_loop+0x10e>
   959cd:	49 8b 54 24 f8       	mov    -0x8(%r12),%rdx
   959d2:	48 85 d2             	test   %rdx,%rdx
   959d5:	74 e9                	je     959c0 <cellbox::run_session_loop+0xa50>
   959d7:	49 8b 3c 24          	mov    (%r12),%rdi
   959db:	be 01 00 00 00       	mov    $0x1,%esi
   959e0:	e8 3b d3 04 00       	call   e2d20 <<linked_list_allocator::Heap>::deallocate>
   959e5:	eb d9                	jmp    959c0 <cellbox::run_session_loop+0xa50>
   959e7:	4c 89 e0             	mov    %r12,%rax
   959ea:	49 83 fc 08          	cmp    $0x8,%r12
   959ee:	0f 83 cc f9 ff ff    	jae    953c0 <cellbox::run_session_loop+0x450>
   959f4:	e9 05 fa ff ff       	jmp    953fe <cellbox::run_session_loop+0x48e>
   959f9:	48 85 d2             	test   %rdx,%rdx
   959fc:	74 0d                	je     95a0b <cellbox::run_session_loop+0xa9b>
   959fe:	be 01 00 00 00       	mov    $0x1,%esi
   95a03:	4c 89 ff             	mov    %r15,%rdi
   95a06:	e8 15 d3 04 00       	call   e2d20 <<linked_list_allocator::Heap>::deallocate>
   95a0b:	4d 85 e4             	test   %r12,%r12
   95a0e:	48 8d ac 24 80 00 00 	lea    0x80(%rsp),%rbp
   95a15:	00 
   95a16:	66 89 5c 24 0e       	mov    %bx,0xe(%rsp)
   95a1b:	75 3a                	jne    95a57 <cellbox::run_session_loop+0xae7>
   95a1d:	eb 82                	jmp    959a1 <cellbox::run_session_loop+0xa31>
   95a1f:	e8 1c cd f7 ff       	call   12740 <cellbox::executor::close_session>
   95a24:	66 89 44 24 0e       	mov    %ax,0xe(%rsp)
   95a29:	66 83 f8 ff          	cmp    $0xffff,%ax
   95a2d:	74 3d                	je     95a6c <cellbox::run_session_loop+0xafc>
   95a2f:	48 8b 54 24 10       	mov    0x10(%rsp),%rdx
   95a34:	48 85 d2             	test   %rdx,%rdx
   95a37:	74 0d                	je     95a46 <cellbox::run_session_loop+0xad6>
   95a39:	be 01 00 00 00       	mov    $0x1,%esi
   95a3e:	4c 89 ff             	mov    %r15,%rdi
   95a41:	e8 da d2 04 00       	call   e2d20 <<linked_list_allocator::Heap>::deallocate>
   95a46:	4d 85 e4             	test   %r12,%r12
   95a49:	48 8d ac 24 80 00 00 	lea    0x80(%rsp),%rbp
   95a50:	00 
   95a51:	0f 84 4a ff ff ff    	je     959a1 <cellbox::run_session_loop+0xa31>
   95a57:	be 01 00 00 00       	mov    $0x1,%esi
   95a5c:	4c 89 ef             	mov    %r13,%rdi
   95a5f:	4c 89 e2             	mov    %r12,%rdx
   95a62:	e8 b9 d2 04 00       	call   e2d20 <<linked_list_allocator::Heap>::deallocate>
   95a67:	e9 35 ff ff ff       	jmp    959a1 <cellbox::run_session_loop+0xa31>
   95a6c:	b8 3c 00 00 00       	mov    $0x3c,%eax
   95a71:	4c 89 f7             	mov    %r14,%rdi
   95a74:	31 f6                	xor    %esi,%esi
   95a76:	31 d2                	xor    %edx,%edx
   95a78:	45 31 d2             	xor    %r10d,%r10d
   95a7b:	0f 05                	syscall
   95a7d:	0f 1f 00             	nopl   (%rax)
   95a80:	b8 68 00 00 00       	mov    $0x68,%eax
   95a85:	31 ff                	xor    %edi,%edi
   95a87:	31 f6                	xor    %esi,%esi
   95a89:	31 d2                	xor    %edx,%edx
   95a8b:	45 31 d2             	xor    %r10d,%r10d
   95a8e:	0f 05                	syscall
   95a90:	eb ee                	jmp    95a80 <cellbox::run_session_loop+0xb10>
   95a92:	bf 18 00 00 00       	mov    $0x18,%edi
   95a97:	e8 84 38 05 00       	call   e9320 <ostd::heap::alloc_error>
   95a9c:	48 8d 35 2d f2 1e 00 	lea    0x1ef22d(%rip),%rsi        # 284cd0 <cellbox::shell_state::ONESHOT+0x1250>
   95aa3:	bf 38 00 00 00       	mov    $0x38,%edi
   95aa8:	e8 03 a7 03 00       	call   d01b0 <core::panicking::panic>
   95aad:	48 85 c0             	test   %rax,%rax
   95ab0:	b8 20 02 00 00       	mov    $0x220,%eax
   95ab5:	be 80 02 00 00       	mov    $0x280,%esi
   95aba:	48 0f 44 f0          	cmove  %rax,%rsi
   95abe:	ba 08 00 00 00       	mov    $0x8,%edx
   95ac3:	e8 38 38 05 00       	call   e9300 <__rustc::__rust_dealloc>
   95ac8:	48 8d 3d e1 0a 21 00 	lea    0x210ae1(%rip),%rdi        # 2a65b0 <memchr::arch::x86_64::memchr::memchr3_raw::FN+0x1f1a8>
   95acf:	e8 cc a6 03 00       	call   d01a0 <core::option::unwrap_failed>
   95ad4:	0f 0b                	ud2
