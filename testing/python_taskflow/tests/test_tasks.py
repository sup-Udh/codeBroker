import unittest
from controllers.TaskController import TaskController

class TestTasks(unittest.TestCase):
    def test_get_task(self):
        controller = TaskController()
        result = controller.get_task(1)
        self.assertEqual(result["id"], 1)

if __name__ == "__main__":
    unittest.main()
