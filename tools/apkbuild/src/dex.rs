//! A small DEX file writer. Android only binds an input method to a class that extends
//! `InputMethodService`, so the APK needs a `classes.dex`. Every method is `native` (implemented
//! in Rust over JNI) except constructors and the static initialisers that load the library, which
//! take a handful of Dalvik instructions. Format reference:
//! <https://source.android.com/docs/core/runtime/dex-format>.

use std::collections::{BTreeMap, BTreeSet};

use crate::hash::{adler32, sha1};

pub const ACC_PUBLIC: u32 = 0x1;
pub const ACC_STATIC: u32 = 0x8;
pub const ACC_FINAL: u32 = 0x10;
pub const ACC_NATIVE: u32 = 0x100;
pub const ACC_CONSTRUCTOR: u32 = 0x10000;
const NO_INDEX: u32 = 0xFFFF_FFFF;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct MethodRef {
    pub class: String,
    pub name: String,
    pub ret: String,
    pub params: Vec<String>,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct FieldRef {
    pub class: String,
    pub name: String,
    pub ty: String,
}

/// The few instructions the stubs need.
#[derive(Clone, Debug)]
pub enum Insn {
    /// `const-string vA, "text"`
    ConstString(u8, String),
    /// `invoke-static {regs}, method`
    InvokeStatic(Vec<u8>, MethodRef),
    /// `invoke-direct {regs}, method`
    InvokeDirect(Vec<u8>, MethodRef),
    /// `iput-wide vA, vB(object), field`
    IputWide(u8, u8, FieldRef),
    ReturnVoid,
}

#[derive(Clone, Debug)]
pub struct Code {
    pub registers: u16,
    pub ins: u16,
    pub outs: u16,
    pub insns: Vec<Insn>,
}

#[derive(Clone, Debug)]
pub struct Method {
    pub name: String,
    pub ret: String,
    pub params: Vec<String>,
    pub access: u32,
    pub code: Option<Code>,
}

#[derive(Clone, Debug)]
pub struct Field {
    pub name: String,
    pub ty: String,
    pub access: u32,
}

#[derive(Clone, Debug)]
pub struct Class {
    pub name: String,
    pub superclass: String,
    pub interfaces: Vec<String>,
    pub access: u32,
    pub fields: Vec<Field>,
    pub methods: Vec<Method>,
}

impl Class {
    pub fn method_ref(&self, method: &Method) -> MethodRef {
        MethodRef { class: self.name.clone(), name: method.name.clone(), ret: method.ret.clone(), params: method.params.clone() }
    }
}

fn shorty(ret: &str, params: &[String]) -> String {
    std::iter::once(ret)
        .chain(params.iter().map(String::as_str))
        .map(|t| if t.starts_with(['L', '[']) { 'L' } else { t.as_bytes()[0] as char })
        .collect()
}

/// Number of 16-bit registers a type takes.
fn width(ty: &str) -> u16 {
    if ty == "J" || ty == "D" {
        2
    } else {
        1
    }
}

/// The number of argument registers a method needs (`this` included unless static).
pub fn ins_size(params: &[String], is_static: bool) -> u16 {
    params.iter().map(|p| width(p)).sum::<u16>() + u16::from(!is_static)
}

type Proto = (String, Vec<String>);

/// Every string, type, prototype, field and method the classes mention, in DEX sort order.
#[derive(Default)]
struct Pools {
    strings: BTreeSet<String>,
    types: BTreeSet<String>,
    protos: BTreeSet<Proto>,
    fields: BTreeSet<FieldRef>,
    methods: BTreeSet<MethodRef>,
}

impl Pools {
    fn add_type(&mut self, ty: &str) {
        self.strings.insert(ty.to_string());
        self.types.insert(ty.to_string());
    }

    fn add_proto(&mut self, ret: &str, params: &[String]) {
        self.strings.insert(shorty(ret, params));
        self.add_type(ret);
        for p in params {
            self.add_type(p);
        }
        self.protos.insert((ret.to_string(), params.to_vec()));
    }

    fn add_method(&mut self, m: &MethodRef) {
        self.add_type(&m.class);
        self.strings.insert(m.name.clone());
        self.add_proto(&m.ret, &m.params);
        self.methods.insert(m.clone());
    }

    fn add_field(&mut self, f: &FieldRef) {
        self.add_type(&f.class);
        self.add_type(&f.ty);
        self.strings.insert(f.name.clone());
        self.fields.insert(f.clone());
    }
}

/// Final indices. Type, field and method order in DEX is by index, and since the string pool is
/// sorted, sorting by string content gives the same order.
struct Indices {
    strings: BTreeMap<String, u32>,
    types: BTreeMap<String, u32>,
    protos: BTreeMap<Proto, u32>,
    fields: BTreeMap<FieldRef, u32>,
    methods: BTreeMap<MethodRef, u32>,
}

/// DEX sorts strings by UTF-16 code units; for the ASCII names used here that is byte order.
fn index<T: Ord + Clone>(items: &[T]) -> BTreeMap<T, u32> {
    items.iter().enumerate().map(|(i, item)| (item.clone(), i as u32)).collect()
}

struct Writer {
    out: Vec<u8>,
}

impl Writer {
    fn u16(&mut self, v: u16) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }
    fn u32(&mut self, v: u32) {
        self.out.extend_from_slice(&v.to_le_bytes());
    }
    fn uleb(&mut self, mut v: u32) {
        loop {
            let byte = (v & 0x7F) as u8;
            v >>= 7;
            if v == 0 {
                self.out.push(byte);
                return;
            }
            self.out.push(byte | 0x80);
        }
    }
    fn align4(&mut self) {
        while !self.out.len().is_multiple_of(4) {
            self.out.push(0);
        }
    }
    fn patch_u32(&mut self, at: usize, v: u32) {
        self.out[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    fn pos(&self) -> u32 {
        self.out.len() as u32
    }
}

fn encode(insns: &[Insn], idx: &Indices) -> Vec<u16> {
    let mut units = Vec::new();
    let invoke = |units: &mut Vec<u16>, op: u16, regs: &[u8], method: &MethodRef| {
        assert!(regs.len() <= 5 && regs.iter().all(|&r| r < 16), "invoke registers out of range");
        let reg = |i: usize| regs.get(i).copied().unwrap_or(0) as u16;
        units.push(((regs.len() as u16) << 12) | (reg(4) << 8) | op);
        units.push(idx.methods[method] as u16);
        units.push((reg(3) << 12) | (reg(2) << 8) | (reg(1) << 4) | reg(0));
    };
    for insn in insns {
        match insn {
            Insn::ConstString(reg, text) => {
                units.push(((*reg as u16) << 8) | 0x1a);
                units.push(idx.strings[text] as u16);
            }
            Insn::InvokeDirect(regs, method) => invoke(&mut units, 0x70, regs, method),
            Insn::InvokeStatic(regs, method) => invoke(&mut units, 0x71, regs, method),
            Insn::IputWide(src, obj, field) => {
                assert!(*src < 16 && *obj < 16);
                units.push(((*obj as u16) << 12) | ((*src as u16) << 8) | 0x5a);
                units.push(idx.fields[field] as u16);
            }
            Insn::ReturnVoid => units.push(0x000e),
        }
    }
    units
}

/// Serialises the classes into a complete `classes.dex`.
pub fn write(classes: &[Class]) -> Vec<u8> {
    let mut pools = Pools::default();
    for class in classes {
        pools.add_type(&class.name);
        pools.add_type(&class.superclass);
        for i in &class.interfaces {
            pools.add_type(i);
        }
        for f in &class.fields {
            pools.add_field(&FieldRef { class: class.name.clone(), name: f.name.clone(), ty: f.ty.clone() });
        }
        for m in &class.methods {
            pools.add_method(&class.method_ref(m));
            for insn in m.code.iter().flat_map(|c| &c.insns) {
                match insn {
                    Insn::ConstString(_, s) => {
                        pools.strings.insert(s.clone());
                    }
                    Insn::InvokeDirect(_, r) | Insn::InvokeStatic(_, r) => pools.add_method(r),
                    Insn::IputWide(_, _, f) => pools.add_field(f),
                    Insn::ReturnVoid => {}
                }
            }
        }
    }
    assert!(pools.strings.iter().all(|s| s.is_ascii()), "only ASCII strings are supported");

    let strings: Vec<String> = pools.strings.iter().cloned().collect();
    let types: Vec<String> = pools.types.iter().cloned().collect();
    let string_idx = index(&strings);
    let type_idx = index(&types);
    // Prototypes sort by return type index, then by parameter type indices.
    let mut protos: Vec<Proto> = pools.protos.iter().cloned().collect();
    protos.sort_by_key(|(ret, params)| (type_idx[ret], params.iter().map(|p| type_idx[p]).collect::<Vec<_>>()));
    let proto_idx = index(&protos);
    let mut fields: Vec<FieldRef> = pools.fields.iter().cloned().collect();
    fields.sort_by_key(|f| (type_idx[&f.class], string_idx[&f.name], type_idx[&f.ty]));
    let field_idx = index(&fields);
    let mut methods: Vec<MethodRef> = pools.methods.iter().cloned().collect();
    methods.sort_by_key(|m| (type_idx[&m.class], string_idx[&m.name], proto_idx[&(m.ret.clone(), m.params.clone())]));
    let method_idx = index(&methods);
    let idx = Indices { strings: string_idx, types: type_idx, protos: proto_idx, fields: field_idx, methods: method_idx };

    let header_size = 0x70u32;
    let string_ids_off = header_size;
    let type_ids_off = string_ids_off + 4 * strings.len() as u32;
    let proto_ids_off = type_ids_off + 4 * types.len() as u32;
    let field_ids_off = proto_ids_off + 12 * protos.len() as u32;
    let method_ids_off = field_ids_off + 8 * fields.len() as u32;
    let class_defs_off = method_ids_off + 8 * methods.len() as u32;
    let data_off = class_defs_off + 32 * classes.len() as u32;

    // The data section is written first into its own buffer, positioned at data_off.
    let mut w = Writer { out: vec![0; data_off as usize] };

    // code_items
    let code_off = w.pos();
    let mut code_offsets: BTreeMap<MethodRef, u32> = BTreeMap::new();
    let mut code_count = 0;
    for class in classes {
        for m in &class.methods {
            if let Some(code) = &m.code {
                w.align4();
                code_offsets.insert(class.method_ref(m), w.pos());
                let units = encode(&code.insns, &idx);
                w.u16(code.registers);
                w.u16(code.ins);
                w.u16(code.outs);
                w.u16(0); // tries
                w.u32(0); // debug info
                w.u32(units.len() as u32);
                for unit in units {
                    w.u16(unit);
                }
                code_count += 1;
            }
        }
    }

    // type_lists: prototype parameters and class interfaces, deduplicated.
    w.align4();
    let type_list_off = w.pos();
    let mut type_lists: BTreeMap<Vec<String>, u32> = BTreeMap::new();
    let wanted = protos.iter().map(|(_, p)| p.clone()).chain(classes.iter().map(|c| c.interfaces.clone()));
    for list in wanted {
        if list.is_empty() || type_lists.contains_key(&list) {
            continue;
        }
        w.align4();
        type_lists.insert(list.clone(), w.pos());
        w.u32(list.len() as u32);
        for t in &list {
            w.u16(idx.types[t] as u16);
        }
    }

    // string_data_items
    let string_data_off = w.pos();
    let mut string_offsets = Vec::new();
    for s in &strings {
        string_offsets.push(w.pos());
        w.uleb(s.len() as u32);
        w.out.extend_from_slice(s.as_bytes());
        w.out.push(0);
    }

    // class_data_items
    let class_data_off = w.pos();
    let mut class_data_offsets = Vec::new();
    for class in classes {
        class_data_offsets.push(w.pos());
        let field_index = |f: &Field| idx.fields[&FieldRef { class: class.name.clone(), name: f.name.clone(), ty: f.ty.clone() }];
        let mut static_fields: Vec<(u32, u32)> =
            class.fields.iter().filter(|f| f.access & ACC_STATIC != 0).map(|f| (field_index(f), f.access)).collect();
        let mut instance_fields: Vec<(u32, u32)> =
            class.fields.iter().filter(|f| f.access & ACC_STATIC == 0).map(|f| (field_index(f), f.access)).collect();
        let is_direct = |m: &Method| m.access & (ACC_STATIC | ACC_CONSTRUCTOR) != 0 || m.name.starts_with('<');
        let entry =
            |m: &Method| (idx.methods[&class.method_ref(m)], m.access, code_offsets.get(&class.method_ref(m)).copied().unwrap_or(0));
        let mut direct: Vec<(u32, u32, u32)> = class.methods.iter().filter(|m| is_direct(m)).map(entry).collect();
        let mut virtual_: Vec<(u32, u32, u32)> = class.methods.iter().filter(|m| !is_direct(m)).map(entry).collect();
        static_fields.sort();
        instance_fields.sort();
        direct.sort();
        virtual_.sort();
        w.uleb(static_fields.len() as u32);
        w.uleb(instance_fields.len() as u32);
        w.uleb(direct.len() as u32);
        w.uleb(virtual_.len() as u32);
        for list in [&static_fields, &instance_fields] {
            let mut previous = 0;
            for &(i, access) in list.iter() {
                w.uleb(i - previous);
                w.uleb(access);
                previous = i;
            }
        }
        for list in [&direct, &virtual_] {
            let mut previous = 0;
            for &(i, access, code) in list.iter() {
                w.uleb(i - previous);
                w.uleb(access);
                w.uleb(code);
                previous = i;
            }
        }
    }

    // map_list
    w.align4();
    let map_off = w.pos();
    let mut map: Vec<(u16, u32, u32)> = vec![
        (0x0000, 1, 0),
        (0x0001, strings.len() as u32, string_ids_off),
        (0x0002, types.len() as u32, type_ids_off),
        (0x0003, protos.len() as u32, proto_ids_off),
        (0x0004, fields.len() as u32, field_ids_off),
        (0x0005, methods.len() as u32, method_ids_off),
        (0x0006, classes.len() as u32, class_defs_off),
        (0x2001, code_count, code_off),
        (0x1001, type_lists.len() as u32, type_list_off),
        (0x2002, strings.len() as u32, string_data_off),
        (0x2000, classes.len() as u32, class_data_off),
        (0x1000, 1, map_off),
    ];
    map.retain(|&(_, count, _)| count > 0);
    w.u32(map.len() as u32);
    for (kind, count, offset) in map {
        w.u16(kind);
        w.u16(0);
        w.u32(count);
        w.u32(offset);
    }
    let file_size = w.pos();

    // Index sections, now that every data offset is known.
    let mut at = string_ids_off as usize;
    let mut put = |w: &mut Writer, v: u32| {
        w.patch_u32(at, v);
        at += 4;
    };
    for &offset in &string_offsets {
        put(&mut w, offset);
    }
    for t in &types {
        put(&mut w, idx.strings[t]);
    }
    for (ret, params) in &protos {
        put(&mut w, idx.strings[&shorty(ret, params)]);
        put(&mut w, idx.types[ret]);
        put(&mut w, if params.is_empty() { 0 } else { type_lists[params] });
    }
    for f in &fields {
        put(&mut w, idx.types[&f.class] | (idx.types[&f.ty] << 16));
        put(&mut w, idx.strings[&f.name]);
    }
    for m in &methods {
        put(&mut w, idx.types[&m.class] | (idx.protos[&(m.ret.clone(), m.params.clone())] << 16));
        put(&mut w, idx.strings[&m.name]);
    }
    for (class, &data) in classes.iter().zip(&class_data_offsets) {
        put(&mut w, idx.types[&class.name]);
        put(&mut w, class.access);
        put(&mut w, idx.types[&class.superclass]);
        put(&mut w, if class.interfaces.is_empty() { 0 } else { type_lists[&class.interfaces] });
        put(&mut w, NO_INDEX); // source file
        put(&mut w, 0); // annotations
        put(&mut w, data);
        put(&mut w, 0); // static values
    }

    // Header.
    let out = &mut w.out;
    out[0..8].copy_from_slice(b"dex\n035\0");
    let fields_at = [
        (32, file_size),
        (36, header_size),
        (40, 0x1234_5678),
        (44, 0), // link size
        (48, 0), // link offset
        (52, map_off),
        (56, strings.len() as u32),
        (60, string_ids_off),
        (64, types.len() as u32),
        (68, type_ids_off),
        (72, protos.len() as u32),
        (76, proto_ids_off),
        (80, fields.len() as u32),
        (84, if fields.is_empty() { 0 } else { field_ids_off }),
        (88, methods.len() as u32),
        (92, method_ids_off),
        (96, classes.len() as u32),
        (100, class_defs_off),
        (104, file_size - data_off),
        (108, data_off),
    ];
    for (offset, value) in fields_at {
        out[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    let signature = sha1(&out[32..]);
    out[12..32].copy_from_slice(&signature);
    let checksum = adler32(&out[12..]);
    out[8..12].copy_from_slice(&checksum.to_le_bytes());
    w.out
}

/// The Java side of Min Keyboard: four classes whose behaviour lives in `libminkeyboard.so`.
pub fn min_keyboard_classes() -> Vec<Class> {
    const PACKAGE: &str = "Lorg/convoy/pinyinime/";
    let class = |name: &str| format!("{PACKAGE}{name};");
    let s = |v: &str| v.to_string();
    let native = |name: &str, ret: &str, params: &[&str]| Method {
        name: s(name),
        ret: s(ret),
        params: params.iter().map(|p| s(p)).collect(),
        access: ACC_PUBLIC | ACC_NATIVE,
        code: None,
    };
    let load_library =
        MethodRef { class: s("Ljava/lang/System;"), name: s("loadLibrary"), ret: s("V"), params: vec![s("Ljava/lang/String;")] };
    let clinit = Method {
        name: s("<clinit>"),
        ret: s("V"),
        params: vec![],
        access: ACC_STATIC | ACC_CONSTRUCTOR,
        code: Some(Code {
            registers: 1,
            ins: 0,
            outs: 1,
            insns: vec![Insn::ConstString(0, s("minkeyboard")), Insn::InvokeStatic(vec![0], load_library), Insn::ReturnVoid],
        }),
    };
    // A constructor that passes its arguments straight to the superclass constructor.
    let forwarding_init = |superclass: &str, params: &[&str]| {
        let params: Vec<String> = params.iter().map(|p| s(p)).collect();
        let ins = ins_size(&params, false);
        let super_init = MethodRef { class: s(superclass), name: s("<init>"), ret: s("V"), params: params.clone() };
        Method {
            name: s("<init>"),
            ret: s("V"),
            access: ACC_PUBLIC | ACC_CONSTRUCTOR,
            code: Some(Code {
                registers: ins,
                ins,
                outs: ins,
                insns: vec![Insn::InvokeDirect((0..ins as u8).collect(), super_init), Insn::ReturnVoid],
            }),
            params,
        }
    };

    let service = Class {
        name: class("ConvoyPinyinImeService"),
        superclass: s("Landroid/inputmethodservice/InputMethodService;"),
        interfaces: vec![],
        access: ACC_PUBLIC,
        fields: vec![],
        methods: vec![
            clinit.clone(),
            forwarding_init("Landroid/inputmethodservice/InputMethodService;", &[]),
            native("onCreate", "V", &[]),
            native("onDestroy", "V", &[]),
            native("onCreateInputView", "Landroid/view/View;", &[]),
            native("onStartInput", "V", &["Landroid/view/inputmethod/EditorInfo;", "Z"]),
            native("onStartInputView", "V", &["Landroid/view/inputmethod/EditorInfo;", "Z"]),
            native("onFinishInputView", "V", &["Z"]),
            native("onUpdateSelection", "V", &["I", "I", "I", "I", "I", "I"]),
            native("onTrimMemory", "V", &["I"]),
            native("onLowMemory", "V", &[]),
        ],
    };
    let setup = Class {
        name: class("SetupActivity"),
        superclass: s("Landroid/app/Activity;"),
        interfaces: vec![],
        access: ACC_PUBLIC,
        fields: vec![],
        methods: vec![
            clinit.clone(),
            forwarding_init("Landroid/app/Activity;", &[]),
            native("onCreate", "V", &["Landroid/os/Bundle;"]),
            native("onActivityResult", "V", &["I", "I", "Landroid/content/Intent;"]),
        ],
    };
    let keyboard_view = Class {
        name: class("KeyboardView"),
        superclass: s("Landroid/view/View;"),
        interfaces: vec![],
        access: ACC_PUBLIC | ACC_FINAL,
        fields: vec![],
        methods: vec![
            forwarding_init("Landroid/view/View;", &["Landroid/content/Context;"]),
            native("onDraw", "V", &["Landroid/graphics/Canvas;"]),
            native("onTouchEvent", "Z", &["Landroid/view/MotionEvent;"]),
            native("onMeasure", "V", &["I", "I"]),
        ],
    };
    let callback_name = class("NativeCallback");
    let id_field = FieldRef { class: callback_name.clone(), name: s("id"), ty: s("J") };
    let callback = Class {
        name: callback_name,
        superclass: s("Ljava/lang/Object;"),
        interfaces: vec![
            s("Ljava/lang/Runnable;"),
            s("Landroid/view/View$OnClickListener;"),
            s("Landroid/widget/CompoundButton$OnCheckedChangeListener;"),
            s("Landroid/widget/RadioGroup$OnCheckedChangeListener;"),
            s("Landroid/content/DialogInterface$OnClickListener;"),
        ],
        access: ACC_PUBLIC | ACC_FINAL,
        fields: vec![Field { name: s("id"), ty: s("J"), access: ACC_PUBLIC | ACC_FINAL }],
        methods: vec![
            Method {
                name: s("<init>"),
                ret: s("V"),
                params: vec![s("J")],
                access: ACC_PUBLIC | ACC_CONSTRUCTOR,
                code: Some(Code {
                    registers: 3,
                    ins: 3,
                    outs: 1,
                    insns: vec![
                        Insn::InvokeDirect(
                            vec![0],
                            MethodRef { class: s("Ljava/lang/Object;"), name: s("<init>"), ret: s("V"), params: vec![] },
                        ),
                        Insn::IputWide(1, 0, id_field),
                        Insn::ReturnVoid,
                    ],
                }),
            },
            native("run", "V", &[]),
            native("onClick", "V", &["Landroid/view/View;"]),
            native("onClick", "V", &["Landroid/content/DialogInterface;", "I"]),
            native("onCheckedChanged", "V", &["Landroid/widget/CompoundButton;", "Z"]),
            native("onCheckedChanged", "V", &["Landroid/widget/RadioGroup;", "I"]),
        ],
    };
    vec![service, setup, keyboard_view, callback]
}
