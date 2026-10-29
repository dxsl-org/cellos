
/tmp/cellbox-s10-3d-resume/cand/desktop.kernel.elf:     file format elf64-x86-64


Disassembly of section .text:

ffffffff80051660 <<cellos_kernel::task::cell_session::State>::endpoint>:
ffffffff80051660:	55                   	push   %rbp
ffffffff80051661:	41 57                	push   %r15
ffffffff80051663:	41 56                	push   %r14
ffffffff80051665:	41 55                	push   %r13
ffffffff80051667:	41 54                	push   %r12
ffffffff80051669:	53                   	push   %rbx
ffffffff8005166a:	4c 8b 5e 28          	mov    0x28(%rsi),%r11
ffffffff8005166e:	4d 85 db             	test   %r11,%r11
ffffffff80051671:	0f 84 16 02 00 00    	je     ffffffff8005188d <<cellos_kernel::task::cell_session::State>::endpoint+0x22d>
ffffffff80051677:	48 8b 46 20          	mov    0x20(%rsi),%rax
ffffffff8005167b:	49 83 fb 01          	cmp    $0x1,%r11
ffffffff8005167f:	75 19                	jne    ffffffff8005169a <<cellos_kernel::task::cell_session::State>::endpoint+0x3a>
ffffffff80051681:	4c 39 40 38          	cmp    %r8,0x38(%rax)
ffffffff80051685:	0f 85 02 02 00 00    	jne    ffffffff8005188d <<cellos_kernel::task::cell_session::State>::endpoint+0x22d>
ffffffff8005168b:	4c 39 48 40          	cmp    %r9,0x40(%rax)
ffffffff8005168f:	0f 85 f8 01 00 00    	jne    ffffffff8005188d <<cellos_kernel::task::cell_session::State>::endpoint+0x22d>
ffffffff80051695:	45 31 d2             	xor    %r10d,%r10d
ffffffff80051698:	eb 4a                	jmp    ffffffff800516e4 <<cellos_kernel::task::cell_session::State>::endpoint+0x84>
ffffffff8005169a:	45 31 d2             	xor    %r10d,%r10d
ffffffff8005169d:	0f 1f 00             	nopl   (%rax)
ffffffff800516a0:	4c 89 d3             	mov    %r10,%rbx
ffffffff800516a3:	4d 89 de             	mov    %r11,%r14
ffffffff800516a6:	49 d1 ee             	shr    $1,%r14
ffffffff800516a9:	4d 01 f2             	add    %r14,%r10
ffffffff800516ac:	4d 69 fa 98 00 00 00 	imul   $0x98,%r10,%r15
ffffffff800516b3:	4e 39 44 38 38       	cmp    %r8,0x38(%rax,%r15,1)
ffffffff800516b8:	4c 0f 47 d3          	cmova  %rbx,%r10
ffffffff800516bc:	4d 29 f3             	sub    %r14,%r11
ffffffff800516bf:	49 83 fb 01          	cmp    $0x1,%r11
ffffffff800516c3:	77 db                	ja     ffffffff800516a0 <<cellos_kernel::task::cell_session::State>::endpoint+0x40>
ffffffff800516c5:	4d 69 da 98 00 00 00 	imul   $0x98,%r10,%r11
ffffffff800516cc:	4e 39 44 18 38       	cmp    %r8,0x38(%rax,%r11,1)
ffffffff800516d1:	0f 85 b6 01 00 00    	jne    ffffffff8005188d <<cellos_kernel::task::cell_session::State>::endpoint+0x22d>
ffffffff800516d7:	49 01 c3             	add    %rax,%r11
ffffffff800516da:	4d 39 4b 40          	cmp    %r9,0x40(%r11)
ffffffff800516de:	0f 85 a9 01 00 00    	jne    ffffffff8005188d <<cellos_kernel::task::cell_session::State>::endpoint+0x22d>
ffffffff800516e4:	4d 69 c2 98 00 00 00 	imul   $0x98,%r10,%r8
ffffffff800516eb:	66 bd 08 00          	mov    $0x8,%bp
ffffffff800516ef:	4a 39 54 00 48       	cmp    %rdx,0x48(%rax,%r8,1)
ffffffff800516f4:	0f 85 97 01 00 00    	jne    ffffffff80051891 <<cellos_kernel::task::cell_session::State>::endpoint+0x231>
ffffffff800516fa:	4a 39 4c 00 50       	cmp    %rcx,0x50(%rax,%r8,1)
ffffffff800516ff:	0f 85 8c 01 00 00    	jne    ffffffff80051891 <<cellos_kernel::task::cell_session::State>::endpoint+0x231>
ffffffff80051705:	4e 8b 14 00          	mov    (%rax,%r8,1),%r10
ffffffff80051709:	66 bd 06 00          	mov    $0x6,%bp
ffffffff8005170d:	49 83 fa 01          	cmp    $0x1,%r10
ffffffff80051711:	0f 84 7a 01 00 00    	je     ffffffff80051891 <<cellos_kernel::task::cell_session::State>::endpoint+0x231>
ffffffff80051717:	4e 8d 0c 00          	lea    (%rax,%r8,1),%r9
ffffffff8005171b:	41 f6 81 80 00 00 00 	testb  $0x1,0x80(%r9)
ffffffff80051722:	01 
ffffffff80051723:	0f 84 68 01 00 00    	je     ffffffff80051891 <<cellos_kernel::task::cell_session::State>::endpoint+0x231>
ffffffff80051729:	46 8b 4c 00 7c       	mov    0x7c(%rax,%r8,1),%r9d
ffffffff8005172e:	66 bd 10 00          	mov    $0x10,%bp
ffffffff80051732:	45 85 c9             	test   %r9d,%r9d
ffffffff80051735:	74 1d                	je     ffffffff80051754 <<cellos_kernel::task::cell_session::State>::endpoint+0xf4>
ffffffff80051737:	41 83 f9 01          	cmp    $0x1,%r9d
ffffffff8005173b:	0f 84 4c 01 00 00    	je     ffffffff8005188d <<cellos_kernel::task::cell_session::State>::endpoint+0x22d>
ffffffff80051741:	41 83 f9 02          	cmp    $0x2,%r9d
ffffffff80051745:	0f 85 46 01 00 00    	jne    ffffffff80051891 <<cellos_kernel::task::cell_session::State>::endpoint+0x231>
ffffffff8005174b:	66 bd 11 00          	mov    $0x11,%bp
ffffffff8005174f:	e9 3d 01 00 00       	jmp    ffffffff80051891 <<cellos_kernel::task::cell_session::State>::endpoint+0x231>
ffffffff80051754:	4c 8b 7e 10          	mov    0x10(%rsi),%r15
ffffffff80051758:	4d 85 ff             	test   %r15,%r15
ffffffff8005175b:	0f 84 2c 01 00 00    	je     ffffffff8005188d <<cellos_kernel::task::cell_session::State>::endpoint+0x22d>
ffffffff80051761:	4e 8b 5c 00 58       	mov    0x58(%rax,%r8,1),%r11
ffffffff80051766:	4a 8b 5c 00 60       	mov    0x60(%rax,%r8,1),%rbx
ffffffff8005176b:	46 8b 4c 00 78       	mov    0x78(%rax,%r8,1),%r9d
ffffffff80051770:	48 8b 76 08          	mov    0x8(%rsi),%rsi
ffffffff80051774:	49 83 ff 01          	cmp    $0x1,%r15
ffffffff80051778:	75 1c                	jne    ffffffff80051796 <<cellos_kernel::task::cell_session::State>::endpoint+0x136>
ffffffff8005177a:	4c 39 1e             	cmp    %r11,(%rsi)
ffffffff8005177d:	0f 85 0a 01 00 00    	jne    ffffffff8005188d <<cellos_kernel::task::cell_session::State>::endpoint+0x22d>
ffffffff80051783:	66 bd 07 00          	mov    $0x7,%bp
ffffffff80051787:	48 39 5e 08          	cmp    %rbx,0x8(%rsi)
ffffffff8005178b:	0f 85 00 01 00 00    	jne    ffffffff80051891 <<cellos_kernel::task::cell_session::State>::endpoint+0x231>
ffffffff80051791:	45 31 f6             	xor    %r14d,%r14d
ffffffff80051794:	eb 43                	jmp    ffffffff800517d9 <<cellos_kernel::task::cell_session::State>::endpoint+0x179>
ffffffff80051796:	45 31 f6             	xor    %r14d,%r14d
ffffffff80051799:	4d 89 f4             	mov    %r14,%r12
ffffffff8005179c:	4d 89 fd             	mov    %r15,%r13
ffffffff8005179f:	49 d1 ed             	shr    $1,%r13
ffffffff800517a2:	4d 01 ee             	add    %r13,%r14
ffffffff800517a5:	49 6b ee 58          	imul   $0x58,%r14,%rbp
ffffffff800517a9:	4c 39 1c 2e          	cmp    %r11,(%rsi,%rbp,1)
ffffffff800517ad:	4d 0f 47 f4          	cmova  %r12,%r14
ffffffff800517b1:	4d 29 ef             	sub    %r13,%r15
ffffffff800517b4:	49 83 ff 01          	cmp    $0x1,%r15
ffffffff800517b8:	77 df                	ja     ffffffff80051799 <<cellos_kernel::task::cell_session::State>::endpoint+0x139>
ffffffff800517ba:	4d 6b fe 58          	imul   $0x58,%r14,%r15
ffffffff800517be:	4e 39 1c 3e          	cmp    %r11,(%rsi,%r15,1)
ffffffff800517c2:	0f 85 c5 00 00 00    	jne    ffffffff8005188d <<cellos_kernel::task::cell_session::State>::endpoint+0x22d>
ffffffff800517c8:	49 01 f7             	add    %rsi,%r15
ffffffff800517cb:	66 bd 07 00          	mov    $0x7,%bp
ffffffff800517cf:	49 39 5f 08          	cmp    %rbx,0x8(%r15)
ffffffff800517d3:	0f 85 b8 00 00 00    	jne    ffffffff80051891 <<cellos_kernel::task::cell_session::State>::endpoint+0x231>
ffffffff800517d9:	4d 6b f6 58          	imul   $0x58,%r14,%r14
ffffffff800517dd:	42 83 7c 36 48 00    	cmpl   $0x0,0x48(%rsi,%r14,1)
ffffffff800517e3:	66 bd 10 00          	mov    $0x10,%bp
ffffffff800517e7:	0f 85 a4 00 00 00    	jne    ffffffff80051891 <<cellos_kernel::task::cell_session::State>::endpoint+0x231>
ffffffff800517ed:	8b 74 24 38          	mov    0x38(%rsp),%esi
ffffffff800517f1:	44 89 cd             	mov    %r9d,%ebp
ffffffff800517f4:	f7 d5                	not    %ebp
ffffffff800517f6:	85 ee                	test   %ebp,%esi
ffffffff800517f8:	0f 85 a9 00 00 00    	jne    ffffffff800518a7 <<cellos_kernel::task::cell_session::State>::endpoint+0x247>
ffffffff800517fe:	4c 01 c0             	add    %r8,%rax
ffffffff80051801:	48 83 c0 08          	add    $0x8,%rax
ffffffff80051805:	4c 89 17             	mov    %r10,(%rdi)
ffffffff80051808:	48 8b 30             	mov    (%rax),%rsi
ffffffff8005180b:	4c 8b 40 08          	mov    0x8(%rax),%r8
ffffffff8005180f:	48 89 77 08          	mov    %rsi,0x8(%rdi)
ffffffff80051813:	4c 89 47 10          	mov    %r8,0x10(%rdi)
ffffffff80051817:	48 8b 70 10          	mov    0x10(%rax),%rsi
ffffffff8005181b:	48 89 77 18          	mov    %rsi,0x18(%rdi)
ffffffff8005181f:	48 8b 70 18          	mov    0x18(%rax),%rsi
ffffffff80051823:	48 89 77 20          	mov    %rsi,0x20(%rdi)
ffffffff80051827:	48 8b 70 20          	mov    0x20(%rax),%rsi
ffffffff8005182b:	48 89 77 28          	mov    %rsi,0x28(%rdi)
ffffffff8005182f:	48 8b 70 28          	mov    0x28(%rax),%rsi
ffffffff80051833:	48 89 77 30          	mov    %rsi,0x30(%rdi)
ffffffff80051837:	48 8b 70 30          	mov    0x30(%rax),%rsi
ffffffff8005183b:	48 89 77 38          	mov    %rsi,0x38(%rdi)
ffffffff8005183f:	48 8b 70 38          	mov    0x38(%rax),%rsi
ffffffff80051843:	48 89 77 40          	mov    %rsi,0x40(%rdi)
ffffffff80051847:	48 89 57 48          	mov    %rdx,0x48(%rdi)
ffffffff8005184b:	48 89 4f 50          	mov    %rcx,0x50(%rdi)
ffffffff8005184f:	4c 89 5f 58          	mov    %r11,0x58(%rdi)
ffffffff80051853:	48 89 5f 60          	mov    %rbx,0x60(%rdi)
ffffffff80051857:	48 8b 48 60          	mov    0x60(%rax),%rcx
ffffffff8005185b:	48 8b 50 68          	mov    0x68(%rax),%rdx
ffffffff8005185f:	48 89 4f 68          	mov    %rcx,0x68(%rdi)
ffffffff80051863:	48 89 57 70          	mov    %rdx,0x70(%rdi)
ffffffff80051867:	44 89 4f 78          	mov    %r9d,0x78(%rdi)
ffffffff8005186b:	c7 47 7c 00 00 00 00 	movl   $0x0,0x7c(%rdi)
ffffffff80051872:	c6 87 80 00 00 00 01 	movb   $0x1,0x80(%rdi)
ffffffff80051879:	8b 48 79             	mov    0x79(%rax),%ecx
ffffffff8005187c:	8b 40 7c             	mov    0x7c(%rax),%eax
ffffffff8005187f:	89 87 84 00 00 00    	mov    %eax,0x84(%rdi)
ffffffff80051885:	89 8f 81 00 00 00    	mov    %ecx,0x81(%rdi)
ffffffff8005188b:	eb 0f                	jmp    ffffffff8005189c <<cellos_kernel::task::cell_session::State>::endpoint+0x23c>
ffffffff8005188d:	66 bd 07 00          	mov    $0x7,%bp
ffffffff80051891:	66 89 6f 08          	mov    %bp,0x8(%rdi)
ffffffff80051895:	48 c7 07 ff ff ff ff 	movq   $0xffffffffffffffff,(%rdi)
ffffffff8005189c:	5b                   	pop    %rbx
ffffffff8005189d:	41 5c                	pop    %r12
ffffffff8005189f:	41 5d                	pop    %r13
ffffffff800518a1:	41 5e                	pop    %r14
ffffffff800518a3:	41 5f                	pop    %r15
ffffffff800518a5:	5d                   	pop    %rbp
ffffffff800518a6:	c3                   	ret
ffffffff800518a7:	83 e6 03             	and    $0x3,%esi
ffffffff800518aa:	31 c0                	xor    %eax,%eax
ffffffff800518ac:	44 85 ce             	test   %r9d,%esi
ffffffff800518af:	0f 94 c0             	sete   %al
ffffffff800518b2:	85 f6                	test   %esi,%esi
ffffffff800518b4:	8d 44 40 06          	lea    0x6(%rax,%rax,2),%eax
ffffffff800518b8:	bd 06 00 00 00       	mov    $0x6,%ebp
ffffffff800518bd:	0f 45 e8             	cmovne %eax,%ebp
ffffffff800518c0:	eb cf                	jmp    ffffffff80051891 <<cellos_kernel::task::cell_session::State>::endpoint+0x231>
