-- Some compile options are host/toolchain-specific and differ across
-- platforms, so exclude them to keep the snapshot portable:
--   COMPILER=          host compiler (clang vs gcc vs msvc)
--   MUTEX_             threading backend (PTHREADS on unix, W32 on Windows)
--   ATOMIC_INTRINSICS  =1 with gcc/clang, =0 with msvc on Windows
select * from pragma_compile_options where compile_options not like 'COMPILER=%' and compile_options not like 'MUTEX_%' and compile_options not like 'ATOMIC_INTRINSICS=%' order by 1; -- @snap compile-options
