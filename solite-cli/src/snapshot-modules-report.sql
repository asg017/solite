/**
  * [0] total_modules_count - total # of modules loaded by the snapshot extension
  * [1] missing_modules - JSON array of missing module names that didn't appear in a snapshot
  *
  *
*/
with executed_vtabs as (
  select 
    regex_capture('SCAN\s+(\S+)', p4, 1) as name,
    sum(nexec) as total_executions
  from solite_snapshot_snapped_statement_bytecode_steps 
  where "opcode" = 'Explain'
    and p4 != 'SCAN CONSTANT ROW'
  group by 1
  union all
  select 
    regex_capture('(?i)CREATE\s+VIRTUAL\s+TABLE\s+.*?\s+USING\s+([^\s(]+)', sql, 1) as module_name,
    count(*)
  from sqlite_master 
  where sql like 'create virtual table %using %'
  group by 1
 ),
 stats_by_vtab as (
   select 
    loaded_modules.name,
    total_executions
  from solite_snapshot_loaded_modules as loaded_modules
  left join executed_vtabs on executed_vtabs.name = loaded_modules.name 
 ),
 final as (
  select
    (
      select 
        count(*) 
      from stats_by_vtab
    ) as total_modules_count,
    (
      select 
        json_group_array(name) filter (where total_executions is null) 
      from stats_by_vtab
    ) as missing_modules
)
select * from final