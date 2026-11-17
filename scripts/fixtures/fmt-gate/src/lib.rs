pub fn   add(a:i32,b:i32)->i32{a+b}
#[cfg(test)] mod tests { #[test] fn t(){ assert_eq!(super::add(1,2),3); } }
