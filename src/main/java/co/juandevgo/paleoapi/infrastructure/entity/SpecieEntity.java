package co.juandevgo.paleoapi.infrastructure.entity;

import jakarta.persistence.*;
import jakarta.validation.constraints.NotNull;
import lombok.Data;

import java.util.List;

@Entity
@Table(name = "species")
@Data
public class SpecieEntity {

    @Id
    @GeneratedValue(strategy = GenerationType.UUID)
    private Long id;

    @NotNull
    @Column(unique = true)
    private String name;

    @Column
    @NotNull
    private String group;

    @Column
    private String phonetic;

    @NotNull
    @Column
    private String diet;

    @NotNull
    @Column
    private String size;

    @NotNull
    @Column
    private String timePeriod;

    @NotNull
    @Column
    private List<String> locations;

    @NotNull
    @Column
    private List<String> classification;

    @Column
    private List<String> images;
}