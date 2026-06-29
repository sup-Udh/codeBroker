from .api import ApiClient
from .api import ApiClient as Client # reexport/alias

# Constructors
client = ApiClient()
client.fetch_user()

# Nested chains
class Config:
    def __init__(self):
        self.api = ApiClient()

config = Config()
config.api.db.query("...")

# Aliases
a = config
b = a
b.api.fetch_user()

# Factory functions
def wrap(val):
    return val
    
wrapped = wrap(ApiClient())
wrapped.fetch_user()

# Recursive aliases
x = ApiClient()
y = x
x = y
y.fetch_user()
