from pydantic import BaseModel
from typing import List, Optional
import sqlite3

class User(BaseModel):
    id: int
    username: str
    email: str

def init_db():
    conn = sqlite3.connect("test.db")
    cursor = conn.cursor()
    cursor.execute("""
        CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            username TEXT NOT NULL,
            email TEXT NOT NULL
        )
    """)
    conn.commit()
    conn.close()

def get_users() -> List[User]:
    return [User(id=1, username="test", email="test@test.com")]
