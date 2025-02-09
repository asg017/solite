select 1;

create table t as 
  select value from generate_series(1, 1e7);
select count(*) from t;

create table t2 as 
  select value, usleep(10) as x from generate_series(1, 10);


select * from bytecode('select 1 + 1;');
select * from bytecode('select sqlite_version();');

select substring(1);

select xxx();


select 2;