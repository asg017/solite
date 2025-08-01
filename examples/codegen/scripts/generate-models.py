import json
import re
import sys
import ast
from pathlib import Path
import hashlib
from dataclasses import dataclass
from typing import Union, Literal

def to_camel_case(snake_str):
    return "".join(x.capitalize() for x in snake_str.lower().split("_"))


@dataclass
class Parameter:
    full_name: str
    name: str
    annotated_type: str | None = None

@dataclass
class Column:
    name: str
    origin_database: str | None = None
    origin_table: str | None = None
    origin_column: str | None = None
    decltype: str | None = None

    @staticmethod
    def from_json(data):
        return Column(
            **data,
        )

@dataclass
class Export:
    name: str
    columns: list[Column]
    parameters: list[Parameter]
    sql: str
    result_type: Union[Literal['Void'] | Literal['Rows'] | Literal['Row'] | Literal['Value'] | Literal['List']]
    
    def from_json(data):
        return Export(
            name=data['name'],
            columns=[Column(**column) for column in data["columns"]],
            parameters=[Parameter(**param) for param in data.get('parameters', [])],
            sql=data['sql'], 
            result_type=data.get('result_type', 'Void')
        )
    def return_type_classname(self) -> str | None:
        """Return the class name for the return type, if applicable."""
        class_name = f"{to_camel_case(self.name)}Result"
        # Convert to CamelCase, 1st letter should be uppercase
        class_name = class_name[0].upper() + class_name[1:]
        return class_name
        

@dataclass
class Report:
    setup: list[str]
    exports: list[Export]
    def from_json(data):
        return Report(
            setup=data.get('setup', []),
            exports=[Export.from_json(export) for export in data.get('exports', [])]
        )

def serialize_string(value: str) -> str:
  expr = ast.fix_missing_locations(ast.Expression(ast.Constant(value=value)))
  compile(expr, filename="<ast>", mode="eval")
  return ast.unparse(expr)

def serialize_variable_name(value: str) -> str:
  expr = ast.fix_missing_locations(ast.Expression(ast.Name(id=value, ctx=ast.Load())))
  compile(expr, filename="<ast>", mode="eval")
  return ast.unparse(expr)

def to_snake_case(name: str) -> str:
    """Convert a string to snake_case."""
    name = re.sub(r'(?<!^)(?=[A-Z])', '_', name).lower()
    return name

def py_type_from_annotated_type(annotated_type: str | None) -> str:
   if annotated_type is None:
       return 'Any'
   if annotated_type in ["text", "str"]:
       return 'str'
   if annotated_type in ["int", "integer", "bigint"]:
       return 'int'
   if annotated_type in ["real", "float"]:
       return 'float'
   if annotated_type in ["blob"]:
       return 'bytes'
   return 'Any'

def py_type_from_decltype(decltype: str | None) -> str:
   if decltype is None:
       return 'Any'
   if decltype in ["TEXT"]:
       return 'str'
   if decltype in ["INT", "INTEGER", "BIGINT"]:
       return 'int'
   if decltype in ["REAL", "FLOAT"]:
       return 'float'
   if decltype in ["BLOB"]:
       return 'bytes'
   # TODO other types
   return 'Any'

def generate_code(report: Report):
    functions = []
    classes = {}

    for idx, export in enumerate(report.exports):

        if len(export.columns) > 0:
          class_name = export.return_type_classname()
          if export.result_type in ['Rows', 'Row']:
            classes[class_name] = [col for col in export.columns]

        func_name = serialize_variable_name(to_snake_case(export.name))
        
        func_lines = []
        sql_literal = serialize_string(export.sql)

        arguments = ''
        for p in export.parameters:
          arguments += f", {serialize_variable_name(to_snake_case(p.name))}: {py_type_from_annotated_type(p.annotated_type)}" 
        
        py_return_type = None
        match export.result_type:
          case 'Void':
            py_return_type = 'None'
          case 'Rows':
            assert class_name is not None
            py_return_type = f'list[{class_name}]'
          case 'Row':
            assert class_name is not None
            py_return_type = f'{class_name} | None'
          case 'Value':
            
            py_return_type = py_type_from_decltype(export.columns[0].decltype)
          case 'List':
            py_return_type = 'list[{}]'.format(py_type_from_decltype(export.columns[0].decltype))
        
        func_lines.append(f"\n  def {func_name}(self{arguments}) -> {py_return_type}:")
        func_lines.append(f'    sql = {sql_literal}')
        if len(export.parameters) > 0:
          gen_params = '{'
          gen_params += ', '.join([f'{serialize_string(to_snake_case(param.full_name[1:]))}: {serialize_variable_name(to_snake_case(param.name))}' for param in export.parameters])
          gen_params += '}'
          func_lines.append(f'    params = {gen_params}')
        else:
          func_lines.append('    params = ()')  
        
        if export.result_type != 'Void':
          func_lines.append('    result = self.connection.execute(sql, params)')
        else:
          func_lines.append('    self.connection.execute(sql, params)')
        match export.result_type:
          case 'Void':
            pass
          case 'Rows':
            func_lines.append(f'    return [{class_name}(*row) for row in result.fetchall()]')
          case 'Row':
            func_lines.append(f'    return {class_name}(*(result.fetchone())) if result else None')
          case 'Value':
            func_lines.append('    row = result.fetchone()')  
            func_lines.append('    return row[0] if row else None')
          case 'List':
            func_lines.append('    return [row[0] for row in result.fetchall()]')
        
        #if class_name is not None:
        #  func_lines.append(f'    return [{class_name}(*row) for row in results.fetchall()]')
        #else:
        #  func_lines.append('    return')
        functions.append('\n'.join(func_lines))

    lines = [
       "import sqlite3\n",
        "from typing import Any\n",
      ]
    if len(classes) > 0:
       lines.append("from dataclasses import dataclass\n")

    # define all the Result dataclasses
    for class_name, columns in classes.items():
        lines.append(f"@dataclass\nclass {class_name}:")
        for c in columns:
            lines.append(f"  {serialize_variable_name(c.name)}: {py_type_from_decltype(c.decltype)}")  # assuming int for simplicity
        lines.append('')
    # define the Db class
    lines.append("class Db:")
    lines.append("  def __init__(self, *kwargs):")
    lines.append("    self.connection = sqlite3.connect(*kwargs)")
    lines.append(f"    sql = {serialize_string(";".join(report.setup))}")
    lines.append("    self.connection.executescript(sql)\n")
    lines.append("  def __enter__(self):")
    lines.append("    self.connection = self.connection.__enter__()")
    lines.append("    return self")
    lines.append("")
    lines.append("  def __exit__(self, exc_type, exc_value, traceback):")
    lines.append("    return self.connection.__exit__(exc_type, exc_value, traceback)")

    # defin the Db class methods
    lines.extend(functions)

    return '\n'.join(lines)


def main():
    if not sys.stdin.isatty():
        data = json.loads(sys.stdin.read())
    else:
        if len(sys.argv) != 2:
            print("Usage: python gen.py <input.json>")
            sys.exit(1)
        input_path = Path(sys.argv[1])
        with input_path.open() as f:
            data = json.load(f)
    report = Report.from_json(data)

    output = generate_code(report)
    hash = hashlib.sha256(output.encode()).hexdigest()
    print(f"# hash: {hash}")
    print(output)

if __name__ == '__main__':
    main()
