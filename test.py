source = "function generateRoomId(): string {\n  return \"x\";\n}\n\nexport async function GET(request: Request) {\n  const id = generateRoomId();\n  return id;\n}\n"
print(len(source))
