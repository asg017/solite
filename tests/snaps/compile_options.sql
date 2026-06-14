-- COMPILER= reports the host compiler (clang vs gcc vs msvc) and MUTEX_=
-- reports the threading backend (PTHREADS on unix, W32 on Windows); both
-- differ across platforms, so exclude them to keep the snapshot portable.
select * from pragma_compile_options where compile_options not like 'COMPILER=%' and compile_options not like 'MUTEX_%' order by 1; -- @snap compile-options
