from fastapi import Request
from starlette.middleware.base import BaseHTTPMiddleware
from app.utils.token_utils import decode_access_token

class AuthMiddleware(BaseHTTPMiddleware):
    async def dispatch(self, request: Request, call_next):
        # Simulated middleware
        response = await call_next(request)
        return response
