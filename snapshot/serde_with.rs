#[macro_export]
macro_rules! serde_map_as_vec {
    (mod $mod:ident, $map:ident<$name:ty, $elem:ty>, $elem_name:ident) => {
        pub mod $mod {
            use ::serde::ser::{Serialize, Serializer};
            use ::serde::de::{Deserialize, Deserializer};
            use super::*;

            pub fn serialize<S: Serializer>(
                map: &$map::<$name, $elem>,
                serializer: S,
            ) -> Result<S::Ok, S::Error> {
                let vec = map.values().collect::<Vec<&$elem>>();
                vec.serialize(serializer)
            }

            pub fn deserialize<'de, D: Deserializer<'de>>(
                deserializer: D,
            ) -> Result<$map<$name, $elem>, D::Error> {
                let vec = Vec::<$elem>::deserialize(deserializer)?;
                let map = vec.into_iter().map(|e| (e.$elem_name.clone(), e))
                    .collect::<$map<$name, $elem>>();
                Ok(map)
            }
        }
    }
}

#[macro_export]
macro_rules! serde_map_as_tuples {
    (mod $mod:ident, $map:ident<$key:ty, $value:ty>) => {
        pub mod $mod {
            use ::serde::ser::{Serialize, Serializer};
            use ::serde::de::{Deserialize, Deserializer};
            use super::*;

            pub fn serialize<S: Serializer>(
                map: &$map<$key, $value>,
                serializer: S,
            ) -> Result<S::Ok, S::Error> {
                let vec = map.iter().collect::<Vec<(&$key, &$value)>>();
                vec.serialize(serializer)
            }

            pub fn deserialize<'de, D: Deserializer<'de>>(
                deserializer: D,
            ) -> Result<$map<$key, $value>, D::Error> {
                let vec = Vec::<($key, $value)>::deserialize(deserializer)?;
                let map = vec.into_iter().collect::<$map<$key, $value>>();
                Ok(map)
            }
        }
    }
}

