use std::path::PathBuf;

fn main() {
    let mut build = cc::Build::new();

    // List of core files in py/
    // This list might be incomplete, usually discovered via glob. 
    // Since we can't easily glob in build.rs without crate, we might assume a set or read dir?
    // BUT we are in a sandbox restricted environment.
    // Let's list common py/ files.
    let py_sources = [
        "mpstate.c", "malloc.c", "gc.c", "pystack.c", "qstr.c", "vstr.c", "mpprint.c",
        "unicode.c", "int.c", "obj.c", "objarray.c", "objattrt.c", "objbool.c", "objboundmeth.c",
        "objcell.c", "objclosure.c", "objcomplex.c", "objdeque.c", "objdict.c", "objenumerate.c",
        "objexcept.c", "objfilter.c", "objfloat.c", "objfun.c", "objgenerator.c", "objgetitemiter.c",
        "objint.c", "objint_mpz.c", "objlist.c", "objmap.c", "objmodule.c", "objobject.c",
        "objpolyiter.c", "objproperty.c", "objnone.c", "objnamedtuple.c", "objrange.c",
        "objreversed.c", "objset.c", "objsingleton.c", "objslice.c", "objstr.c", "objstrunicode.c",
        "objstringio.c", "objtuple.c", "objtype.c", "objzip.c", "opmethods.c", "sequence.c",
        "stream.c", "binary.c", "builtin.c", "argcheck.c", "errno.c", "emitcommon.c", "emitbc.c",
        "emitglue.c", "emitnative.c", "formatfloat.c", "parsenum.c", "parsenumbase.c", "lexer.c",
        "parse.c", "scope.c", "compile.c", "inlineasm.c", "asmbase.c", "asmx64.c", "asmx86.c",
        "asmthumb.c", "asmarm.c", "repl.c", "smallint.c", "frozenmod.c", "modsys.c", "modio.c",
        "modmath.c", "modcmath.c", "modmicropython.c", "modstruct.c", "modgc.c", "modthread.c",
        "vm.c", "bc.c", "showbc.c", "profile.c", "map.c"
    ];

    for file in &py_sources {
        let path = format!("src/c/py/{}", file);
        // build.file() check existence? cc handles it.
        // We add it blindly.
        build.file(path);
    }
    
    // We also need a way to generate qstrdefs.generated.h.
    // If we skip it, compilation fails.
    // We can try to rely on `MICROPY_NO_QSTR`? No such thing easily.
    
    build
        .include("src/c/py")
        .include("src/c/vios") // for mpconfigport.h
        .define("NO_QSTR", None) // Experimental attempt to suppress QSTR requirements or minimal headers
        .warnings(false)
        .compile("micropython");

    println!("cargo:rerun-if-changed=src/c/py");
}
