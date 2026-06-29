from .db import Database, create_db

class ApiClient:
    def __init__(self):
        self.db = create_db()
        
    def fetch_user(self):
        return self.db.query("SELECT * FROM users")
