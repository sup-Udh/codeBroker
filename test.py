from storage import Database

class TestService:
    def __init__(self, db: Database):
        self.db = db

    def do_something(self):
        print("Doing something in Python!")
