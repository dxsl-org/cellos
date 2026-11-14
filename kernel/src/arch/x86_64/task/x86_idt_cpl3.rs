use crate::task::stack::Stack;
use crate::task::tcb::TaskState;
use crate::task::tcb::WaitKind;
use core::sync::atomic::{AtomicUsize, Ordering};
use types::CellId;

static A_TID: AtomicUsize = AtomicUsize::new(0);
static B_TID: AtomicUsize = AtomicUsize::new(0);

fn fail() -> ! {
    crate::hal::idt::cpl3_probe_fail()
}

fn map_user_code() -> (usize, usize, usize, usize) {
    use crate::memory::paging::Flags;
    let (image, a_offset, b_offset, b_return_offset) = crate::hal::idt::cpl3_user_image();
    if image.len() > crate::memory::paging::PAGE_SIZE {
        fail();
    }
    let code = crate::memory::frame::allocate_frame().unwrap_or_else(|| fail());
    let code_virt = crate::memory::frame::phys_to_virt(code);
    unsafe {
        core::ptr::write_bytes(code_virt as *mut u8, 0, crate::memory::paging::PAGE_SIZE);
        core::ptr::copy_nonoverlapping(image.as_ptr(), code_virt as *mut u8, image.len());
    }
    let flags = Flags::from_bits(
        Flags::VALID | Flags::READ | Flags::EXECUTE | Flags::USER | Flags::ACCESSED,
    );
    let mapped = crate::memory::frame::with(|allocator| {
        crate::memory::paging::map_page(allocator, code, code, flags).is_ok()
    });
    if mapped != Some(true) {
        fail();
    }
    (code, code + a_offset, code + b_offset, b_return_offset)
}

pub(crate) fn run() -> ! {
    crate::hal::idt::require_cpl3_pku();
    let (code, a_entry, b_entry, b_return_offset) = map_user_code();
    let pages = crate::task::stack_pages_for("x86-idt-cpl3");
    let b_kstack = Stack::new_kernel(pages).unwrap_or_else(|_| fail());
    let b_ustack = Stack::new_user(pages).unwrap_or_else(|_| fail());
    let a_kstack = Stack::new_kernel(pages).unwrap_or_else(|_| fail());
    let a_ustack = Stack::new_user(pages).unwrap_or_else(|_| fail());

    let mut guard = crate::task::SCHEDULER.lock();
    let scheduler = guard.as_mut().unwrap_or_else(|| fail());
    if !scheduler.tasks.is_empty() || crate::sched::hart_local::ready::total_ready_count() != 0 {
        fail();
    }
    let b_tid = scheduler
        .spawn_with_stacks_configured(
            "x86-idt-cpl3-b",
            CellId(0x7ff0),
            alloc::vec::Vec::new(),
            b_kstack,
            b_ustack,
            |task| {
                task.pku_value = crate::hal::idt::CPL3_PKRU_B;
                crate::task::prime_user_mode_entry(task, b_entry, 0);
            },
        )
        .unwrap_or_else(|_| fail());
    let a_tid = scheduler
        .spawn_with_stacks_configured(
            "x86-idt-cpl3-a",
            CellId(0x7ff1),
            alloc::vec::Vec::new(),
            a_kstack,
            a_ustack,
            |task| {
                task.pku_value = crate::hal::idt::CPL3_PKRU_A;
                crate::task::prime_user_mode_entry(task, a_entry, 0);
            },
        )
        .unwrap_or_else(|_| fail());
    B_TID.store(b_tid, Ordering::Release);
    A_TID.store(a_tid, Ordering::Release);
    drop(guard);

    crate::hal::idt::arm_cpl3_probe(code, b_return_offset);
    crate::sched::smp::x86_idt_fixture_uniprocessor_ready();
    crate::task::yield_cpu();
    fail()
}

fn park_current(expected: usize) {
    if expected == 0 || crate::task::current_task_id() != expected {
        fail();
    }
    let mut guard = crate::task::SCHEDULER.lock();
    let scheduler = guard.as_mut().unwrap_or_else(|| fail());
    let task = scheduler.tasks.get_mut(&expected).unwrap_or_else(|| fail());
    task.state = TaskState::Blocked {
        wait: WaitKind::Join { target: 0 },
        deadline: None,
    };
}

#[no_mangle]
pub extern "Rust" fn vi_x86_idt_cpl3_park_b() {
    park_current(B_TID.load(Ordering::Acquire));
    crate::task::yield_cpu();
}

#[no_mangle]
pub extern "Rust" fn vi_x86_idt_cpl3_wake_b() {
    let b_tid = B_TID.load(Ordering::Acquire);
    if crate::task::current_task_id() != A_TID.load(Ordering::Acquire) {
        fail();
    }
    let mut guard = crate::task::SCHEDULER.lock();
    let scheduler = guard.as_mut().unwrap_or_else(|| fail());
    let task = scheduler.tasks.get_mut(&b_tid).unwrap_or_else(|| fail());
    if !matches!(
        task.state,
        TaskState::Blocked {
            wait: WaitKind::Join { .. },
            ..
        }
    ) {
        fail();
    }
    task.state = TaskState::Ready;
    scheduler.push_ready(b_tid);
}

#[no_mangle]
pub extern "Rust" fn vi_x86_idt_cpl3_switch_to_a() {
    let b_tid = B_TID.load(Ordering::Acquire);
    park_current(b_tid);
    let a_tid = A_TID.load(Ordering::Acquire);
    let ready = crate::task::SCHEDULER
        .lock()
        .as_ref()
        .and_then(|scheduler| scheduler.tasks.get(&a_tid))
        .is_some_and(|task| task.state == TaskState::Ready);
    if !ready {
        fail();
    }
    crate::task::yield_cpu();
}
