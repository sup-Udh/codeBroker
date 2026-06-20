import unittest
from controllers.ProjectController import ProjectController

class TestProjects(unittest.TestCase):
    def test_get_project(self):
        controller = ProjectController()
        result = controller.get_project(1)
        self.assertEqual(result["id"], 1)

if __name__ == "__main__":
    unittest.main()
