import requests
from bs4 import BeautifulSoup

url = "https://modern.ircdocs.horse/formatting.html"
req = requests.get(url)
soup = BeautifulSoup(req.content, 'html.parser')

table = soup.find_all(class_="rgb-table")[0]
for cell in table.find_all("td"):
    hexcode = cell.find(class_="hexcode")
    colorcode = cell.find(class_="colorcode")
    if hexcode and colorcode:
        print(f"(Rgb::from_hex(0x{hexcode.string}), {colorcode.string}),")
