# hash: 95641d1507eb716d9641ff73b6c3f5d984df1eaf4f3a5d73ea01d9308b9dab04
import sqlite3

from typing import Any

from dataclasses import dataclass

@dataclass
class HighschoolstudentsResult:
  id: int
  name: str
  age: int
  grade: int

@dataclass
class LookupstudentResult:
  id: int
  name: str
  age: int
  grade: int

@dataclass
class Lookupstudent24Result:
  id: int
  name: str
  age: int
  grade: int

class Db:
  def __init__(self, *kwargs):
    self.connection = sqlite3.connect(*kwargs)
    sql = 'create table students(\n    id integer primary key autoincrement,\n    name text not null,\n    age integer not null,\n    grade integer not null\n);'
    self.connection.executescript(sql)

  def __enter__(self):
    self.connection = self.connection.__enter__()
    return self

  def __exit__(self, exc_type, exc_value, traceback):
    return self.connection.__exit__(exc_type, exc_value, traceback)

  def high_school_students(self) -> list[HighschoolstudentsResult]:
    sql = 'select * from students\nwhere grade between 9 and 12;'
    params = ()
    result = self.connection.execute(sql, params)
    return [HighschoolstudentsResult(*row) for row in result.fetchall()]

  def lookup_student(self, query: Any) -> list[LookupstudentResult]:
    sql = 'select * from students\nwhere name like :query;'
    params = {'query': query}
    result = self.connection.execute(sql, params)
    return [LookupstudentResult(*row) for row in result.fetchall()]

  def insert_student(self, name: Any, age: Any, grade: Any) -> None:
    sql = 'insert into students (name, age, grade)\nvalues (:name, :age, :grade);'
    params = {'name': name, 'age': age, 'grade': grade}
    self.connection.execute(sql, params)

  def lookup_student24(self, query: str) -> list[Lookupstudent24Result]:
    sql = 'select * from students\nwhere name like $query::text;'
    params = {'query::text': query}
    result = self.connection.execute(sql, params)
    return [Lookupstudent24Result(*row) for row in result.fetchall()]

  def distinct_grades(self) -> list[int]:
    sql = 'select distinct grade from students order by 1;'
    params = ()
    result = self.connection.execute(sql, params)
    return [row[0] for row in result.fetchall()]
