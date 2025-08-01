create table students(
    id integer primary key autoincrement,
    name text not null,
    age integer not null,
    grade integer not null
);


-- name: highSchoolStudents
select * from students
where grade between 9 and 12;

-- name: lookupStudent
select * from students
where name like :query;

-- name: insertStudent
insert into students (name, age, grade)
values (:name, :age, :grade);

-- name: lookupStudent24
select * from students
where name like $query::text;


-- name: distinctGrades :list
select distinct grade from students order by 1;