#![allow(non_snake_case, non_upper_case_globals, unused_variables)]

pub mod dynload;
#[cfg(not(target_os = "android"))]
pub mod embedded;
pub mod sys_std;
pub mod sys_unix;
pub mod system;
#[cfg(not(target_os = "android"))]
pub mod x11;
