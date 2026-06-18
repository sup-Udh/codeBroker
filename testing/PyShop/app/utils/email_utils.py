from app.config.settings import settings

def send_email_sync(to_email: str, subject: str, body: str):
    print(f"Connecting to {settings.EMAIL_HOST}:{settings.EMAIL_PORT}...")
    print(f"Sending email to {to_email}: {subject}")
    return True
