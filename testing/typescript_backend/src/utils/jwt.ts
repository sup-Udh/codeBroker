import * as crypto from 'crypto';

const SECRET_KEY = 'super-secret-benchmark-key';

export interface TokenPayload {
  userId: string;
  role: string;
  iat?: number;
  exp?: number;
}

export const generateToken = (payload: Omit<TokenPayload, 'iat' | 'exp'>, expiresInMinutes: number = 60): string => {
  const iat = Math.floor(Date.now() / 1000);
  const exp = iat + expiresInMinutes * 60;
  
  const fullPayload: TokenPayload = { ...payload, iat, exp };
  
  const header = Buffer.from(JSON.stringify({ alg: 'HS256', typ: 'JWT' })).toString('base64url');
  const body = Buffer.from(JSON.stringify(fullPayload)).toString('base64url');
  
  const signature = crypto
    .createHmac('sha256', SECRET_KEY)
    .update(`${header}.${body}`)
    .digest('base64url');
    
  return `${header}.${body}.${signature}`;
};

export const verifyToken = (token: string): TokenPayload => {
  const parts = token.split('.');
  if (parts.length !== 3) {
    throw new Error('Invalid token format');
  }
  
  const [header, body, signature] = parts;
  
  const expectedSignature = crypto
    .createHmac('sha256', SECRET_KEY)
    .update(`${header}.${body}`)
    .digest('base64url');
    
  if (signature !== expectedSignature) {
    throw new Error('Invalid signature');
  }
  
  const payload: TokenPayload = JSON.parse(Buffer.from(body, 'base64url').toString('utf8'));
  
  if (payload.exp && payload.exp < Math.floor(Date.now() / 1000)) {
    throw new Error('Token expired');
  }
  
  return payload;
};
