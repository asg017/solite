import unittest
from models import Db


class TestDb(unittest.TestCase):
    def setUp(self):
        """Set up test database with sample data before each test."""
        self.db = Db(':memory:')
        self.db.insert_student(name="Alex", age=16, grade=10)
        self.db.insert_student(name="Jamie", age=17, grade=11)
        self.db.insert_student(name="Taylor", age=15, grade=9)
        self.db.insert_student(name="Jordan", age=8, grade=4)
    
    def tearDown(self):
        """Close the database connection after each test."""
        self.db.connection.close()

    def test_insert_student(self):
        """Test that students are inserted correctly."""
        # Insert a new student
        self.db.insert_student(name="Sam", age=18, grade=12)
        
        # Verify the student was inserted by checking total count
        all_students = self.db.connection.execute("select * from students;").fetchall()
        self.assertEqual(len(all_students), 5)  # 4 from setUp + 1 new
        
        # Verify the new student's data
        sam = self.db.lookup_student("Sam")
        self.assertEqual(len(sam), 1)
        self.assertEqual(sam[0].name, "Sam")
        self.assertEqual(sam[0].age, 18)
        self.assertEqual(sam[0].grade, 12)

    def test_high_school_students(self):
        """Test filtering high school students (grades 9-12)."""
        high_school_students = self.db.high_school_students()
        
        # Should return Alex (grade 10), Jamie (grade 11), and Taylor (grade 9)
        # But not Jordan (grade 4)
        self.assertEqual(len(high_school_students), 3)
        
        # Verify the correct students are returned
        names = [student.name for student in high_school_students]
        self.assertIn("Alex", names)
        self.assertIn("Jamie", names)  
        self.assertIn("Taylor", names)
        self.assertNotIn("Jordan", names)

    def test_lookup_student(self):
        """Test looking up students by name."""
        alex = self.db.lookup_student("Alex")
        
        # Should find exactly one student
        self.assertEqual(len(alex), 1)
        
        # Verify the student data
        self.assertEqual(alex[0].name, "Alex")
        self.assertEqual(alex[0].age, 16)
        self.assertEqual(alex[0].grade, 10)
        
        # Test case-insensitive search (assuming LIKE is case-insensitive)
        alex_lower = self.db.lookup_student("alex")
        self.assertEqual(len(alex_lower), 1)
    def test_lookup_student_2(self):
        result = self.db.lookup_student24('J%')
        
        # Should find Jamie
        self.assertEqual(len(result), 2)
        self.assertEqual(result[0].name, "Jamie")
        self.assertEqual(result[1].name, "Jordan")
    
    def test_all(self):
        self.assertEqual(self.db.distinct_grades(), [4, 9, 10, 11])
        
    def test_lookup_nonexistent_student(self):
        """Test looking up a student that doesn't exist."""
        result = self.db.lookup_student("NonExistent")
        self.assertEqual(len(result), 0)

    def test_database_connection(self):
        """Test that database connection works and data persists."""
        all_students = self.db.connection.execute("select * from students;").fetchall()
        
        # Should have 4 students from setUp
        self.assertEqual(len(all_students), 4)
        
        # Verify each student exists
        student_names = [row[1] for row in all_students]  # name is second column
        expected_names = ["Alex", "Jamie", "Taylor", "Jordan"]
        for name in expected_names:
            self.assertIn(name, student_names)

if __name__ == '__main__':
    unittest.main()