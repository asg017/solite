/**
  * [0] total_functions_count - total # of functions loaded by the snapshot extension
  * [1] missing_functions - JSON array of missing functions that didn't appear in a snapshot
  *
  *
*/
with executed_functions as (
  select 
    substr(p4, 1, instr(p4, '(') - 1) as function_name,
    sum(nexec) as total_executions
  from solite_snapshot_snapped_statement_bytecode_steps 
  where opcode in ('Function', 'AggFinal')
  group by 1
),
stats_by_function as (
  select 
    loaded_functions.name,
    total_executions
  from solite_snapshot_loaded_functions as loaded_functions
  left join executed_functions on loaded_functions.name = executed_functions.function_name
),
final as (
  select
    (
      select 
        count(*) 
      from stats_by_function
    ) as total_functions_count,
    (
      select 
        json_group_array(name) filter (where total_executions is null) 
      from stats_by_function
    ) as missing_functions
)
select * from final