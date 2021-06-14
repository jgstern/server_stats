import { Component, OnInit, ViewChild } from '@angular/core';
import Autolinker from 'autolinker';
import { ApiService, Row } from '../api.service';
import { MatPaginator } from '@angular/material/paginator';
import { MatSort } from '@angular/material/sort';
import { MatTableDataSource } from '@angular/material/table';

@Component({
  selector: 'app-link-finder',
  templateUrl: './link-finder.component.html',
  styleUrls: ['./link-finder.component.scss']
})
export class LinkFinderComponent implements OnInit {
  room_name = "";
  links: any[] = [];
  private rows: Row[] = [];
  dataSource = new MatTableDataSource<Row>([]);
  filterColumn = 'incoming';
  displayedColumns: string[] = ['name', 'alias', 'room_id', 'topic', 'members', 'incoming_links', 'outgoing_links'];
  private first = true;
  resultsLength = 0;
  isLoadingResults = true;

  @ViewChild(MatPaginator, { static: true })
  private paginator!: MatPaginator;
  @ViewChild(MatSort, { static: true })
  private sort!: MatSort;

  constructor(public api: ApiService,) { }

  ngOnInit(): void {
    if (this.api.data != null) {
      this.links = this.api.data.links;
      if (this.first) {
        this.rows = this.api.data.nodes;
        this.rows = this.rows.filter(node => this.links.some(value => node["id"] === value["target"])).map(data => {
          if (data.updated === false || data.updated == null) {
            const alias_server = data.alias.split(":")[1];
            data.alias = `<a href="https://matrix.to/#/${data.alias}?via=${alias_server}&via=matrix.org" target="_blank" rel="noopener noreferrer">${data.alias}</a>`;
            data.topic = Autolinker.link(this.truncateText(data.topic, 500), { sanitizeHtml: true });
            data.updated = true;
          }
          return data;
        });
        this.first = false;
        this.resultsLength = this.rows.length;
        this.isLoadingResults = false;
        this.dataSource.data = this.rows;
        this.dataSource.sort = this.sort;
        this.dataSource.paginator = this.paginator;
      }
    }
    this.api.getDataUpdates().subscribe(data => {
      if (data != null) {
        const nodes = data.nodes;
        this.links = data.links;
        if (this.first) {
          this.rows = nodes;
          this.rows = this.rows.filter(node => this.links.some(value => node["id"] === value["target"])).map(data => {
            if (data.updated === false || data.updated == null) {
              const alias_server = data.alias.split(":")[1];
              data.alias = `<a href="https://matrix.to/#/${data.alias}?via=${alias_server}&via=matrix.org" target="_blank" rel="noopener noreferrer">${data.alias}</a>`;
              data.topic = Autolinker.link(this.truncateText(data.topic, 500), { sanitizeHtml: true });
              data.updated = true;
            }
            return data;
          });
          this.first = false;
          this.resultsLength = this.rows.length;
          this.isLoadingResults = false;
          this.dataSource.data = this.rows;
          this.dataSource.sort = this.sort;
          this.dataSource.paginator = this.paginator;
        }
      }
    })
  }

  truncateText(text: string, length: number) {
    if (text.length <= length) {
      return text;
    }

    return text.substr(0, length) + '\u2026';
  }

  updateSelection(value: string) {
    this.filterColumn = value.toLowerCase();
  }

  updateFilter(event: Event) {
    const filterValue = (event.target as HTMLInputElement).value;
    this.dataSource.data = this.rows;
    this.room_name = "";
    if (filterValue !== "") {
      this.dataSource.filter = filterValue;

      if (this.dataSource.filteredData.length > 0) {
        this.room_name = this.dataSource.filteredData[this.dataSource.filteredData.length - 1].name;
        const room_hash = this.dataSource.filteredData[this.dataSource.filteredData.length - 1].id;
        if (this.filterColumn == "incoming") {
          const links = this.links.filter(link => link["target"] === room_hash);
          const rows = this.rows.filter(node => links.some(value => node["id"] === value["source"])).map(data => {
            if (data.updated === false || data.updated == null) {
              const alias_server = data.alias.split(":")[1];
              data.alias = `<a href="https://matrix.to/#/${data.alias}?via=${alias_server}&via=matrix.org" target="_blank" rel="noopener noreferrer">${data.alias}</a>`;
              data.topic = Autolinker.link(this.truncateText(data.topic, 500), { sanitizeHtml: true });
              data.updated = true;
            }
            return data;
          });
          this.dataSource.data = rows;
          this.dataSource.filter = "";
        } else if (this.filterColumn == "outgoing") {
          const links = this.links.filter(link => link["source"] === room_hash);
          const rows = this.rows.filter(node => links.some(value => node["id"] === value["target"])).map(data => {
            if (data.updated === false || data.updated == null) {
              const alias_server = data.alias.split(":")[1];
              data.alias = `<a href="https://matrix.to/#/${data.alias}?via=${alias_server}&via=matrix.org" target="_blank" rel="noopener noreferrer">${data.alias}</a>`;
              data.topic = Autolinker.link(this.truncateText(data.topic, 500), { sanitizeHtml: true });
              data.updated = true;
            }
            return data;
          });
          this.dataSource.data = rows;
          this.dataSource.filter = "";
        }
      }
    }


  }
}
