/// A special macro to validate keys and assign values to a struct.
#[macro_export]
macro_rules! validate_keys {
    ($name:expr, $map:expr, $($key:ident),*) => {
        let mut seen_keys = std::collections::HashSet::new();

        $map.map(|(key, value)| {
            let key_str = key.as_str();
            // Check for unique keys
            if !seen_keys.insert(key_str) {
                return Err(vec![_partialerror!(
                    *key.span(),
                    ErrorKind::DuplicateKey(key_str.to_string()),
                )]);
            }

            // Check for allowed keys and assign values
            match key_str {
                $(
                    stringify!($key) => {
                        $name.$key = value.try_convert(key_str)?;
                    },
                )*
                _ => {
                    return Err(
                        vec![_partialerror!(
                            *key.span(),
                            ErrorKind::InvalidField(key_str.to_string().into()),
                            help = format!("valid options for {name} are {valid_options}", name = stringify!($name), valid_options = stringify!($($key),*))
                        )]
                    )
                }
            }
            Ok(())
        }).flatten_errors()?;
    };
}
