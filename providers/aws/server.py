"""
Soma Storage Provider — AWS DynamoDB

Setup:
  pip install boto3
  export AWS_ACCESS_KEY_ID=...
  export AWS_SECRET_ACCESS_KEY=...
  export AWS_DEFAULT_REGION=eu-west-1

Run:
  python providers/aws/server.py
  python providers/aws/server.py --port 9100 --prefix myapp_

Tables are auto-created: {prefix}{cell}_{field}
Keys: pk (String partition key), value (String JSON-encoded)
"""

from http.server import HTTPServer, BaseHTTPRequestHandler
import json
import sys
import time

PORT = 9100
PREFIX = ""
for i, arg in enumerate(sys.argv):
    if arg == '--port' and i + 1 < len(sys.argv):
        PORT = int(sys.argv[i + 1])
    if arg == '--prefix' and i + 1 < len(sys.argv):
        PREFIX = sys.argv[i + 1]

try:
    import boto3
    from botocore.exceptions import ClientError
    dynamodb = boto3.resource('dynamodb')
    HAS_BOTO = True
except ImportError:
    print("WARNING: boto3 not installed. Run: pip install boto3")
    HAS_BOTO = False

tables = {}

def table_name(cell, field):
    return f"{PREFIX}{cell}_{field}"

def ensure_table(name):
    if not HAS_BOTO:
        return None
    try:
        table = dynamodb.Table(name)
        table.load()
        return table
    except ClientError as e:
        if e.response['Error']['Code'] == 'ResourceNotFoundException':
            table = dynamodb.create_table(
                TableName=name,
                KeySchema=[{'AttributeName': 'pk', 'KeyType': 'HASH'}],
                AttributeDefinitions=[{'AttributeName': 'pk', 'AttributeType': 'S'}],
                BillingMode='PAY_PER_REQUEST'
            )
            table.wait_until_exists()
            print(f"  created table: {name}")
            return table
        raise

def get_table(cell, field):
    name = table_name(cell, field)
    if name not in tables:
        tables[name] = ensure_table(name)
    return tables[name]

class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
        cell = body.get('cell', '')
        field = body.get('field', '')
        path = self.path

        try:
            if path == '/health':
                self.respond({'status': 'ok', 'provider': 'aws-dynamodb', 'has_boto': HAS_BOTO})
                return

            table = get_table(cell, field)
            if table is None:
                self.respond({'error': 'boto3 not installed'}, status=500)
                return

            if path == '/get':
                resp = table.get_item(Key={'pk': body['key']}, ConsistentRead=True)
                item = resp.get('Item')
                if item and 'value' in item:
                    self.respond({'value': json.loads(item['value'])})
                else:
                    self.respond({'value': None})

            elif path == '/set':
                table.put_item(Item={'pk': body['key'], 'value': json.dumps(body['value'])})
                self.respond({'ok': True})

            elif path == '/delete':
                try:
                    table.delete_item(Key={'pk': body['key']}, ConditionExpression='attribute_exists(pk)')
                    self.respond({'deleted': True})
                except ClientError as e:
                    if e.response['Error']['Code'] == 'ConditionalCheckFailedException':
                        self.respond({'deleted': False})
                    else:
                        raise

            elif path == '/keys':
                items = []
                resp = table.scan(ProjectionExpression='pk')
                items.extend([i['pk'] for i in resp['Items'] if not i['pk'].startswith('__')])
                while 'LastEvaluatedKey' in resp:
                    resp = table.scan(ProjectionExpression='pk', ExclusiveStartKey=resp['LastEvaluatedKey'])
                    items.extend([i['pk'] for i in resp['Items'] if not i['pk'].startswith('__')])
                self.respond({'keys': items})

            elif path == '/values':
                items = []
                resp = table.scan()
                items.extend([json.loads(i['value']) for i in resp['Items'] if 'value' in i])
                self.respond({'values': items})

            elif path == '/has':
                resp = table.get_item(Key={'pk': body['key']}, ProjectionExpression='pk')
                self.respond({'exists': 'Item' in resp})

            elif path == '/len':
                count = 0
                resp = table.scan(Select='COUNT')
                count += resp['Count']
                while 'LastEvaluatedKey' in resp:
                    resp = table.scan(Select='COUNT', ExclusiveStartKey=resp['LastEvaluatedKey'])
                    count += resp['Count']
                self.respond({'len': count})

            elif path == '/append':
                key = f"__log_{int(time.time() * 1000000)}"
                table.put_item(Item={'pk': key, 'value': json.dumps(body['value']), '__is_log': True})
                self.respond({'ok': True})

            elif path == '/list':
                items = []
                resp = table.scan(FilterExpression='attribute_exists(#log)', ExpressionAttributeNames={'#log': '__is_log'})
                items.extend([json.loads(i['value']) for i in resp['Items'] if 'value' in i])
                self.respond({'items': items})

            else:
                self.respond({'error': f'unknown endpoint: {path}'}, status=404)

        except Exception as e:
            print(f"  ERROR: {e}")
            self.respond({'error': str(e)}, status=500)

    def respond(self, data, status=200):
        self.send_response(status)
        self.send_header('Content-Type', 'application/json')
        self.end_headers()
        self.wfile.write(json.dumps(data).encode())

    def log_message(self, format, *args):
        print(f"  {args[0]}")

if __name__ == '__main__':
    print(f"soma AWS provider (DynamoDB)")
    print(f"listening on http://localhost:{PORT}")
    print(f"table prefix: '{PREFIX}'")
    print(f"---")
    HTTPServer(('0.0.0.0', PORT), Handler).serve_forever()
