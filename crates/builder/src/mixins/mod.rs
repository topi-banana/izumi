use crustf::CodeBuilder;

pub mod minecraft_server;

pub use minecraft_server::MinecraftServerMixin;

/// Java-side type of a target-method argument. The single-character descriptor
/// (or `L...;` / `[...`), the stack/local slot size, and the matching load
/// opcode are all derived from this.
#[derive(Debug, Clone, Copy)]
#[allow(dead_code)] // Public set of variants: used by individual Mixins as needed.
pub enum JavaType {
    Int,
    Long,
    Float,
    Double,
    Boolean,
    Byte,
    Short,
    Char,
    /// internal name, e.g. `"java/util/function/BooleanSupplier"`.
    Object(&'static str),
    /// The descriptor remaining after the leading `'['`. For example, `"I"` for
    /// an int array, `"Ljava/lang/String;"` for `String[]`, and `"[I"` for a
    /// two-dimensional int array.
    Array(&'static str),
}

impl JavaType {
    pub fn slot_size(&self) -> u16 {
        match self {
            JavaType::Long | JavaType::Double => 2,
            _ => 1,
        }
    }

    pub fn descriptor(&self) -> String {
        match self {
            JavaType::Int => "I".into(),
            JavaType::Long => "J".into(),
            JavaType::Float => "F".into(),
            JavaType::Double => "D".into(),
            JavaType::Boolean => "Z".into(),
            JavaType::Byte => "B".into(),
            JavaType::Short => "S".into(),
            JavaType::Char => "C".into(),
            JavaType::Object(name) => format!("L{name};"),
            JavaType::Array(inner) => format!("[{inner}"),
        }
    }

    /// Pushes the value at position `slot` onto the operand stack.
    pub fn emit_load(&self, c: &mut CodeBuilder, slot: u16) {
        match self {
            JavaType::Long => {
                c.lload(slot);
            }
            JavaType::Double => {
                c.dload(slot);
            }
            JavaType::Float => {
                c.fload(slot);
            }
            JavaType::Object(_) | JavaType::Array(_) => {
                c.aload(slot);
            }
            JavaType::Int
            | JavaType::Boolean
            | JavaType::Byte
            | JavaType::Short
            | JavaType::Char => {
                c.iload(slot);
            }
        }
    }
}

/// A single native static method placed on `com.izumi.runtime.NativePayloads`.
pub struct NativeMethod {
    pub name: String,
    pub descriptor: String,
}

#[derive(Debug, Clone, Copy)]
pub enum MixinAt {
    Head,
    Return,
}

impl std::fmt::Display for MixinAt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MixinAt::Head => write!(f, "HEAD"),
            MixinAt::Return => write!(f, "RETURN"),
        }
    }
}

/// A single @Inject handler method on the generated Mixin class.
pub struct MixinMethod {
    pub name: &'static str,
    pub target_method: &'static str,
    /// The target method's argument list. Combined with a trailing
    /// `CallbackInfo`, it uniquely determines both the handler and native
    /// descriptors.
    pub target_args: &'static [JavaType],
    pub at: MixinAt,
    pub cancellable: bool,
    pub exceptions: &'static [&'static str],
    /// Name of the native static method placed on the NativePayloads holder
    /// class (= the Rust-side `#[inject]` function name).
    pub native_name: &'static str,
    pub code: fn(&MixinMethod, &dyn MixinClass, &mut CodeBuilder),
}

impl MixinMethod {
    /// Descriptor used by both the Mixin handler and the native static method.
    /// Order: the target_args followed by CallbackInfo. The return type is void.
    pub fn descriptor(&self) -> String {
        let mut s = String::from("(");
        for t in self.target_args {
            s.push_str(&t.descriptor());
        }
        s.push_str("Lorg/spongepowered/asm/mixin/injection/callback/CallbackInfo;)V");
        s
    }
}

pub trait MixinClass: Sync {
    fn target_class(&self) -> &'static str;

    fn target_class_descriptor(&self) -> String {
        format!("L{};", self.target_class())
    }

    fn mixin_class_simple_name(&self) -> &'static str;

    /// Name of the corresponding cdylib (= `[[example]] name`). The builder
    /// expects `target/release/examples/{prefix}<name>{suffix}`.
    fn native_lib_name(&self) -> &'static str;

    fn methods(&self) -> &'static [MixinMethod];

    /// Lists native_name + descriptor from `methods()`, deduplicated. Even when
    /// multiple @Inject handlers point at the same payload (e.g. `cancel_demo`),
    /// deduping on the `(name, descriptor)` pair means NativePayloads emits only
    /// one native declaration.
    fn native_methods(&self) -> Vec<NativeMethod> {
        let mut seen: std::collections::BTreeSet<(String, String)> =
            std::collections::BTreeSet::new();
        let mut out = Vec::new();
        for m in self.methods() {
            let desc = m.descriptor();
            let key = (m.native_name.to_string(), desc.clone());
            if seen.insert(key) {
                out.push(NativeMethod {
                    name: m.native_name.to_string(),
                    descriptor: desc,
                });
            }
        }
        out
    }
}
