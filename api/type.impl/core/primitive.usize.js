(function() {
    var type_impls = Object.fromEntries([["cellos_kernel",[]],["hal_core",[]],["hal_riscv",[]],["ostd",[]],["types",[]]]);
    if (window.register_type_impls) {
        window.register_type_impls(type_impls);
    } else {
        window.pending_type_impls = type_impls;
    }
})()
//{"start":55,"fragment_lengths":[20,16,17,12,13]}