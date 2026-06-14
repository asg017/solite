-- COMPILER= reports the host compiler (clang vs gcc), which differs across
-- platforms; exclude it so the snapshot is portable.
select * from pragma_compile_options where compile_options not like 'COMPILER=%' order by 1; -- @snap compile-options
