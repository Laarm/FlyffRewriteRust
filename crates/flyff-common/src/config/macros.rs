//! The macros behind the single key table of [`tables`](super::tables) (Plan/03 §4.1, WP-CORE-90).
//!
//! One invocation of [`config_tables!`] defines, from one list of key rows:
//!
//! * the five section structs (`Features`, `Gameplay`, `Rates`, `EventDefaults`, `Fidelity`) with their
//!   `Default` impls, serde attributes and one doc line per key,
//! * [`KEY_REGISTRY`](super::KEY_REGISTRY), the one [`KeyMeta`](super::KeyMeta) per key,
//! * the typed browser projection of every key with the C flag (`ClientFeatures`, `ClientGameplay`,
//!   `ClientFidelity`) and the `SimFlags` projection of every key whose side is `Sim`,
//! * `RulesConfig`, the aggregate of the five sections.
//!
//! Adding a key is one new row in the table; the registry, the projections, the template, the generated
//! TypeScript and the docs follow without a second list (a test cross-checks the projections).
//!
//! Row syntax (every row ends with `;`):
//!
//! ```text
//! name: Type = default ; Class Side client|- reserved|- ; "legacy ref" ; "owner doc" ; ledger ; "doc" ;
//! ```

/// `true` for the `client` / `reserved` row flags.
macro_rules! row_flag {
    (client) => {
        true
    };
    (reserved) => {
        true
    };
    (-) => {
        false
    };
}

/// Entry point: the list of sections.
macro_rules! config_tables {
    ($($sec:ident $sname:ident $cname:tt { $($entry:tt)* })*) => {
        $( config_section! { $sec $sname $cname { $($entry)* } } )*
        config_registry! { $( $sec { $($entry)* } )* }
        config_sim! { $( $sec { $($entry)* } )* }
        config_rules! { $( $sec $sname )* }
    };
}

/// One section: struct, defaults, name-based bool accessors and the optional client projection.
macro_rules! config_section {
    (
        $sec:ident $sname:ident $cname:tt {
            $(
                $name:ident : $ty:ty = $def:expr ;
                $class:ident $side:ident $client:tt $reserved:tt ;
                $legacy:literal ; $owner:literal ; $ledger:expr ; $doc:literal ;
            )*
        }
    ) => {
        #[doc = concat!("The `[", stringify!($sec), "]` section (Plan/03 §4; every key has a default).")]
        #[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
        #[serde(default, deny_unknown_fields)]
        pub struct $sname {
            $(
                #[doc = $doc]
                #[serde(skip_serializing_if = "crate::config::macros::config_skip::is_none_like")]
                pub $name: $ty,
            )*
        }

        impl Default for $sname {
            fn default() -> Self {
                Self { $( $name: $def, )* }
            }
        }

        impl $sname {
            /// The value of the boolean key `name` of this section; `None` for an unknown key or a
            /// key of another type.
            pub fn bool_value(&self, name: &str) -> Option<bool> {
                match name {
                    $( stringify!($name) => (&self.$name as &dyn core::any::Any).downcast_ref::<bool>().copied(), )*
                    _ => None,
                }
            }

            /// Sets the boolean key `name`; `false` for an unknown key or a key of another type.
            pub fn set_bool(&mut self, name: &str, value: bool) -> bool {
                match name {
                    $(
                        stringify!($name) => match (&mut self.$name as &mut dyn core::any::Any).downcast_mut::<bool>() {
                            Some(slot) => {
                                *slot = value;
                                true
                            }
                            None => false,
                        },
                    )*
                    _ => false,
                }
            }
        }

        config_section! { @client $cname $sname [] $( ($name : $ty ; $client) )* }
    };

    // No browser projection for this section.
    (@client - $sname:ident [] $($rest:tt)*) => {};
    // Fold the accumulated fields into the struct.
    (@client $cname:ident $sname:ident [$( $f:ident : $t:ty , )*]) => {
        #[doc = concat!("Browser projection of [`", stringify!($sname), "`]: exactly the keys with the C flag (Plan/03 §4.1).")]
        #[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
        pub struct $cname {
            $( #[doc = concat!("`", stringify!($f), "`")] pub $f: $t, )*
        }

        impl From<&$sname> for $cname {
            fn from(src: &$sname) -> Self {
                Self { $( $f: src.$f.clone(), )* }
            }
        }
    };
    (@client $cname:ident $sname:ident [$($acc:tt)*] ($name:ident : $ty:ty ; client) $($rest:tt)*) => {
        config_section! { @client $cname $sname [$($acc)* $name : $ty ,] $($rest)* }
    };
    (@client $cname:ident $sname:ident [$($acc:tt)*] ($name:ident : $ty:ty ; -) $($rest:tt)*) => {
        config_section! { @client $cname $sname [$($acc)*] $($rest)* }
    };
}

/// `KEY_REGISTRY`: one `KeyMeta` per row, in table order.
macro_rules! config_registry {
    (
        $(
            $sec:ident {
                $(
                    $name:ident : $ty:ty = $def:expr ;
                    $class:ident $side:ident $client:tt $reserved:tt ;
                    $legacy:literal ; $owner:literal ; $ledger:expr ; $doc:literal ;
                )*
            }
        )*
    ) => {
        /// Exactly one entry per key of Plan/03 §4.2–§4.7 and §5.2 (generated from the key table).
        pub static KEY_REGISTRY: &[KeyMeta] = &[
            $( $(
                KeyMeta {
                    key: concat!(stringify!($sec), ".", stringify!($name)),
                    section: stringify!($sec),
                    name: stringify!($name),
                    class: KeyClass::$class,
                    side: KeySide::$side,
                    client_delivered: row_flag!($client),
                    legacy: $legacy,
                    owner_doc: $owner,
                    ledger: $ledger,
                    reserved: row_flag!($reserved),
                    doc: $doc,
                    ty: stringify!($ty),
                },
            )* )*
        ];
    };
}

/// `SimFlags`: every key whose side is `Sim`, flattened across sections.
macro_rules! config_sim {
    (
        $(
            $sec:ident {
                $(
                    $name:ident : $ty:ty = $def:expr ;
                    $class:ident $side:ident $client:tt $reserved:tt ;
                    $legacy:literal ; $owner:literal ; $ledger:expr ; $doc:literal ;
                )*
            }
        )*
    ) => {
        config_sim! { @acc [] $( $( ($sec $name : $ty ; $side) )* )* }
    };
    (@acc [$( ($sec:ident $name:ident : $ty:ty) )*]) => {
        /// Projection consumed by `flyff-sim` on BOTH sides: every key whose side is `Sim` (Plan/03 §4.1).
        /// The server builds it from the config; the browser receives it with the client features.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub struct SimFlags {
            $( #[doc = concat!("`", stringify!($sec), ".", stringify!($name), "`")] pub $name: $ty, )*
        }

        impl SimFlags {
            /// Builds the flags from the aggregate configuration.
            pub fn from_rules(rules: &RulesConfig) -> Self {
                Self { $( $name: rules.$sec.$name, )* }
            }
        }

        impl Default for SimFlags {
            fn default() -> Self {
                Self::from_rules(&RulesConfig::default())
            }
        }

        impl From<&RulesConfig> for SimFlags {
            fn from(rules: &RulesConfig) -> Self {
                Self::from_rules(rules)
            }
        }
    };
    (@acc [$($acc:tt)*] ($sec:ident $name:ident : $ty:ty ; Sim) $($rest:tt)*) => {
        config_sim! { @acc [$($acc)* ($sec $name : $ty)] $($rest)* }
    };
    (@acc [$($acc:tt)*] ($sec:ident $name:ident : $ty:ty ; $other:ident) $($rest:tt)*) => {
        config_sim! { @acc [$($acc)*] $($rest)* }
    };
}

/// `RulesConfig`: the five sections side by side.
macro_rules! config_rules {
    ($( $sec:ident $sname:ident )*) => {
        /// The five sections owned by Plan/03 (`[features]`, `[gameplay]`, `[rates]`, `[events]`,
        /// `[fidelity]`) as one value; `flyff-server`'s `Config` embeds them flat.
        #[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
        #[serde(default, deny_unknown_fields)]
        pub struct RulesConfig {
            $( #[doc = concat!("`[", stringify!($sec), "]`.")] pub $sec: $sname, )*
        }

        impl RulesConfig {
            /// The boolean value of the dotted key `section.name`; `None` when the key is unknown or
            /// not a boolean.
            pub fn bool_by_key(&self, key: &str) -> Option<bool> {
                let (section, name) = key.split_once('.')?;
                match section {
                    $( stringify!($sec) => self.$sec.bool_value(name), )*
                    _ => None,
                }
            }

            /// Sets the boolean dotted key; `false` when the key is unknown or not a boolean.
            pub fn set_bool_by_key(&mut self, key: &str, value: bool) -> bool {
                let Some((section, name)) = key.split_once('.') else {
                    return false;
                };
                match section {
                    $( stringify!($sec) => self.$sec.set_bool(name, value), )*
                    _ => false,
                }
            }
        }
    };
}

/// Serde helper: `Option` keys that are `None` are not serialised (TOML has no null).
pub(crate) mod config_skip {
    /// Skips `None`; serialises everything else.
    pub(crate) fn is_none_like<T: 'static>(value: &T) -> bool {
        (value as &dyn core::any::Any)
            .downcast_ref::<Option<f32>>()
            .is_some_and(Option::is_none)
    }
}
