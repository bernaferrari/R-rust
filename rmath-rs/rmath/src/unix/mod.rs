#![allow(non_snake_case, non_upper_case_globals, unused_variables)]

pub mod dynload;
#[cfg(not(target_os = "android"))]
pub mod embedded;
#[cfg(not(target_os = "android"))]
pub mod sys_std;
#[cfg(not(target_os = "android"))]
pub mod sys_unix;
#[cfg(not(target_os = "android"))]
pub mod system;
#[cfg(not(target_os = "android"))]
pub mod x11;
