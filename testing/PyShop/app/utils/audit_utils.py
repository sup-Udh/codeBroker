def format_audit_message(action: str, resource: str, user_id: int) -> str:
    return f"User {user_id} performed {action} on {resource}"
