use log::{info, warn, error};
use alloc::vec::Vec;
use alloc::vec;
use alloc::boxed::Box;
use alloc::string::String;
use alloc::collections::BTreeMap;
use crate::prelude::*;

/// Represents a loaded executable unit in the system (Theseus-style "Cell").
pub struct Cell {
    pub id: u32,
    pub name: String,
    pub kind: CellKind,
    pub memory_range: (usize, usize), 
    pub dependencies: Vec<String>,    
    pub exports: BTreeMap<String, usize>, 
    pub unresolved_imports: Vec<String>,  
}

#[derive(Debug, PartialEq)]
pub enum CellKind {
    NativeObject, 
    WasmModule,   
}

pub struct RuntimeLinker {
    cells: BTreeMap<String, Box<Cell>>,
    next_cell_id: u32,
}

impl Default for RuntimeLinker {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeLinker {
    pub fn new() -> Self {
        Self {
            cells: BTreeMap::new(),
            next_cell_id: 1,
        }
    }

    pub fn load_cell(&mut self, name: &str, kind: CellKind, exports: Vec<(&str, usize)>, imports: Vec<&str>) -> Result<&Cell, String> {
        info!("Linker: Loading Cell '{}' ({:?})...", name, kind);
        let mut cell = Box::new(Cell {
            id: self.next_cell_id,
            name: String::from(name),
            kind,
            memory_range: (0x1000 * self.next_cell_id as usize, 0x1000 * (self.next_cell_id as usize + 1)), 
            dependencies: Vec::new(),
            exports: BTreeMap::new(),
            unresolved_imports: Vec::new(),
        });
        for (sym, addr) in exports {
            cell.exports.insert(String::from(sym), addr);
        }
        for imp in imports {
            if self.resolve_symbol(imp).is_none() {
                cell.unresolved_imports.push(String::from(imp));
                warn!("Linker: Cell '{}' has unresolved import '{}'", name, imp);
            }
        }
        self.next_cell_id += 1;
        if name.starts_with("vios-driver-") {
            let id = crate::process::drivers::register_driver(name); 
            info!("Linker: Auto-registered driver '{}' as ID {}", name, id);
        }
        self.cells.insert(String::from(name), cell);
        Ok(self.cells.get(name).unwrap())
    }

    pub fn resolve_symbol(&self, symbol: &str) -> Option<usize> {
        for cell in self.cells.values() {
            if let Some(&addr) = cell.exports.get(symbol) {
                return Some(addr);
            }
        }
        None
    }
}

static mut LINKER: Option<RuntimeLinker> = None;

pub fn init() {
    info!("Loader: Initializing Runtime Linker (Theseus Style)...");
    
    unsafe {
        LINKER = Some(RuntimeLinker::new());
        let linker = LINKER.as_mut().unwrap();

        let core_exports = vec![
             ("vios_alloc", 0x1000),
             ("vios_print", 0x1020),
        ];
        let _ = linker.load_cell("core", CellKind::NativeObject, core_exports, vec![]);

        let driver_imports = vec!["vios_alloc", "vios_print"];
        match linker.load_cell("vios-driver-motor", CellKind::NativeObject, vec![], driver_imports) {
            Ok(cell) => {
               let id = crate::process::spawn(&cell.name, alloc::vec::Vec::new());
               if let Some(sched) = crate::process::SCHEDULER.lock().as_mut() {
                   if let Some(task) = sched.tasks.get_mut(&id) {
                       let entry = vios_driver_motor::driver_main as *const () as usize;
                       let (gp, tp) = crate::process::get_kernel_gp_tp();
                       task.context.ra = entry;
                       task.context.mepc = entry;
                       task.context.mstatus = 0x1800; // MPP=M-mode
                       task.context.gp = gp;
                       task.context.tp = tp;
                       info!("Loader: Linked Task '{}' (ID {}) to vios_driver_motor::driver_main (0x{:X})", cell.name, id, entry);
                   }
               }
            }
            Err(e) => error!("Loader: Failed to load driver: {}", e),
        }

        // Step 4: Load User Shell (Disabled for isolation)
        /*
        if let Ok(cell) = linker.load_cell("vios-shell", CellKind::NativeObject, vec![], vec!["vios_print"]) {
              let mut permissions = alloc::vec::Vec::new();
              if let Some(motor_id) = crate::process::drivers::resolve("vios-driver-motor") {
                  permissions.push(motor_id);
              }
              // crate::process::spawn(&cell.name, permissions);
        }
        */
        
        info!("Loader: === Skipping IPC Tests (Isolation for Debugging) ===");
    }
}
