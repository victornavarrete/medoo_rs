/// Construye un `Vec<(String, Value)>` listo para `InsertQuery::set`.
///
/// ```ignore
/// let row = record!{ "name" => "ana", "age" => 30 };
/// db.insert("users").set(row);
/// ```
#[macro_export]
macro_rules! record {
    ( $( $col:expr => $val:expr ),* $(,)? ) => {{
        let v: Vec<(String, $crate::Value)> = vec![
            $( ( $col.to_string(), $crate::IntoValue::into_value($val) ) ),*
        ];
        v
    }};
}

/// Construye un `Cond::And` a partir de comparaciones planas estilo Medoo.
///
/// ```ignore
/// let c = where_!{ "age" => [">", 18], "status" => "active" };
/// let c = where_!{ "deleted_at" => null, "name" => not_null };
/// ```
#[macro_export]
macro_rules! where_ {
    ( $( $col:expr => $rhs:tt ),* $(,)? ) => {{
        let mut __conds: Vec<$crate::Cond> = Vec::new();
        $( $crate::__where_one!(__conds, $col, $rhs); )*
        $crate::Cond::And(__conds)
    }};
}

#[macro_export]
#[doc(hidden)]
macro_rules! __where_one {
    ($acc:ident, $col:expr, [ $op:expr, $val:expr ]) => {{
        $acc.push($crate::Cond::op($col, $op, $val).expect("medoo_rs: operador inválido en where!"));
    }};
    ($acc:ident, $col:expr, null) => {{
        $acc.push($crate::Cond::IsNull { col: $col.to_string(), negate: false });
    }};
    ($acc:ident, $col:expr, not_null) => {{
        $acc.push($crate::Cond::IsNull { col: $col.to_string(), negate: true });
    }};
    ($acc:ident, $col:expr, $val:expr) => {{
        $acc.push($crate::Cond::eq($col, $val));
    }};
}
